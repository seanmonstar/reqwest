use std::fmt;
use std::net::SocketAddr;
use std::pin::Pin;
use std::time::{Duration, Instant};

use bytes::Bytes;
use encoding_rs::{Encoding, UTF_8};
use futures_util::{StreamExt, TryStreamExt};
use hyper::client::connect::HttpInfo;
use hyper::{HeaderMap, StatusCode, Version};
use mime::Mime;
#[cfg(feature = "json")]
use serde::de::DeserializeOwned;
#[cfg(feature = "json")]
use serde_json;
use tokio::time::Sleep;
use url::Url;

use super::body::Body;
use super::decoder::{Accepts, Decoder};
#[cfg(feature = "cookies")]
use crate::cookie;
use crate::response::ResponseUrl;

/// Response configuration.
///
/// Configured per `Client` with
/// [`ClientBuilder::response_config()`](struct.ClientBuilder.html#method.response_config)
#[derive(Debug, Clone, Default)]
pub struct ResponseConfig {
    /// Setting this to `Some(speed_limit_config)` aborts
    /// any **full** response body retrieval whenever the
    /// download speed is lower than `speed_limit_config`.
    ///
    /// This setting is in other words only used whenever [`Response::bytes()`]
    /// (or indirectly by [`Response::text()`] and [`Response::json()`]) is
    /// being awaited, and **not** by awaiting [`Response::chunk()`] or
    /// [`Response::bytes_stream()`] etc.
    pub lowest_allowed_speed: Option<SpeedLimitConfig>,
}

/// Currently used in [`ResponseConfig`] to optionally
/// place lowest allowed speed limits.
#[derive(Debug, Clone)]
pub struct SpeedLimitConfig {
    bytes_per_second: usize,
    /// ### Invariant:
    /// Must always be greater than one second.
    ///
    /// (Further reading for a more detailed explanation can
    /// be found in the source code of [`Response::bytes()`])
    duration_window: Duration,
}

impl SpeedLimitConfig {
    /// # Errors
    ///
    /// Errors if `duration_window` is less than one second.
    /// This constraint is needed to protect against integer
    /// division by zero during speed calculations.
    pub fn try_new(bytes_per_second: usize, duration_window: Duration) -> crate::Result<Self> {
        if duration_window.as_millis() < 1000 {
            return Err(crate::error::builder(
                "duration window may not be less than one second, this is to avoid division by zero during speed calculations"
            ));
        }

        Ok(Self {
            bytes_per_second,
            duration_window,
        })
    }
}

/// A Response to a submitted `Request`.
pub struct Response {
    pub(super) res: hyper::Response<Decoder>,
    // Boxed to save space (11 words to 1 word), and it's not accessed
    // frequently internally.
    url: Box<Url>,
    response_config: ResponseConfig,
}

impl Response {
    pub(super) fn new(
        res: hyper::Response<hyper::Body>,
        url: Url,
        accepts: Accepts,
        timeout: Option<Pin<Box<Sleep>>>,
        response_config: ResponseConfig,
    ) -> Response {
        let (mut parts, body) = res.into_parts();
        let decoder = Decoder::detect(&mut parts.headers, Body::response(body, timeout), accepts);
        let res = hyper::Response::from_parts(parts, decoder);

        Response {
            res,
            url: Box::new(url),
            response_config,
        }
    }

    /// Get the `StatusCode` of this `Response`.
    #[inline]
    pub fn status(&self) -> StatusCode {
        self.res.status()
    }

    /// Get the HTTP `Version` of this `Response`.
    #[inline]
    pub fn version(&self) -> Version {
        self.res.version()
    }

    /// Get the `Headers` of this `Response`.
    #[inline]
    pub fn headers(&self) -> &HeaderMap {
        self.res.headers()
    }

    /// Get a mutable reference to the `Headers` of this `Response`.
    #[inline]
    pub fn headers_mut(&mut self) -> &mut HeaderMap {
        self.res.headers_mut()
    }

    /// Get the content-length of this response, if known.
    ///
    /// Reasons it may not be known:
    ///
    /// - The server didn't send a `content-length` header.
    /// - The response is compressed and automatically decoded (thus changing
    ///   the actual decoded length).
    pub fn content_length(&self) -> Option<u64> {
        use hyper::body::HttpBody;

        HttpBody::size_hint(self.res.body()).exact()
    }

    /// Retrieve the cookies contained in the response.
    ///
    /// Note that invalid 'Set-Cookie' headers will be ignored.
    ///
    /// # Optional
    ///
    /// This requires the optional `cookies` feature to be enabled.
    #[cfg(feature = "cookies")]
    #[cfg_attr(docsrs, doc(cfg(feature = "cookies")))]
    pub fn cookies<'a>(&'a self) -> impl Iterator<Item = cookie::Cookie<'a>> + 'a {
        cookie::extract_response_cookies(self.res.headers()).filter_map(Result::ok)
    }

    /// Get the final `Url` of this `Response`.
    #[inline]
    pub fn url(&self) -> &Url {
        &self.url
    }

    /// Get the remote address used to get this `Response`.
    pub fn remote_addr(&self) -> Option<SocketAddr> {
        self.res
            .extensions()
            .get::<HttpInfo>()
            .map(|info| info.remote_addr())
    }

    /// Returns a reference to the associated extensions.
    pub fn extensions(&self) -> &http::Extensions {
        self.res.extensions()
    }

    /// Returns a mutable reference to the associated extensions.
    pub fn extensions_mut(&mut self) -> &mut http::Extensions {
        self.res.extensions_mut()
    }

    // body methods

    /// Get the full response text.
    ///
    /// This method decodes the response body with BOM sniffing
    /// and with malformed sequences replaced with the REPLACEMENT CHARACTER.
    /// Encoding is determined from the `charset` parameter of `Content-Type` header,
    /// and defaults to `utf-8` if not presented.
    ///
    /// Note that the BOM is stripped from the returned String.
    ///
    /// # Example
    ///
    /// ```
    /// # async fn run() -> Result<(), Box<dyn std::error::Error>> {
    /// let content = reqwest::get("http://httpbin.org/range/26")
    ///     .await?
    ///     .text()
    ///     .await?;
    ///
    /// println!("text: {:?}", content);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn text(self) -> crate::Result<String> {
        self.text_with_charset("utf-8").await
    }

    /// Get the full response text given a specific encoding.
    ///
    /// This method decodes the response body with BOM sniffing
    /// and with malformed sequences replaced with the REPLACEMENT CHARACTER.
    /// You can provide a default encoding for decoding the raw message, while the
    /// `charset` parameter of `Content-Type` header is still prioritized. For more information
    /// about the possible encoding name, please go to [`encoding_rs`] docs.
    ///
    /// Note that the BOM is stripped from the returned String.
    ///
    /// [`encoding_rs`]: https://docs.rs/encoding_rs/0.8/encoding_rs/#relationship-with-windows-code-pages
    ///
    /// # Example
    ///
    /// ```
    /// # async fn run() -> Result<(), Box<dyn std::error::Error>> {
    /// let content = reqwest::get("http://httpbin.org/range/26")
    ///     .await?
    ///     .text_with_charset("utf-8")
    ///     .await?;
    ///
    /// println!("text: {:?}", content);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn text_with_charset(self, default_encoding: &str) -> crate::Result<String> {
        let content_type = self
            .headers()
            .get(crate::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<Mime>().ok());
        let encoding_name = content_type
            .as_ref()
            .and_then(|mime| mime.get_param("charset").map(|charset| charset.as_str()))
            .unwrap_or(default_encoding);
        let encoding = Encoding::for_label(encoding_name.as_bytes()).unwrap_or(UTF_8);

        let full = self.bytes().await?;

        let (text, _, _) = encoding.decode(&full);
        Ok(text.into_owned())
    }

    /// Try to deserialize the response body as JSON.
    ///
    /// # Optional
    ///
    /// This requires the optional `json` feature enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// # extern crate reqwest;
    /// # extern crate serde;
    /// #
    /// # use reqwest::Error;
    /// # use serde::Deserialize;
    /// #
    /// // This `derive` requires the `serde` dependency.
    /// #[derive(Deserialize)]
    /// struct Ip {
    ///     origin: String,
    /// }
    ///
    /// # async fn run() -> Result<(), Error> {
    /// let ip = reqwest::get("http://httpbin.org/ip")
    ///     .await?
    ///     .json::<Ip>()
    ///     .await?;
    ///
    /// println!("ip: {}", ip.origin);
    /// # Ok(())
    /// # }
    /// #
    /// # fn main() { }
    /// ```
    ///
    /// # Errors
    ///
    /// This method fails whenever the response body is not in JSON format
    /// or it cannot be properly deserialized to target type `T`. For more
    /// details please see [`serde_json::from_reader`].
    ///
    /// [`serde_json::from_reader`]: https://docs.serde.rs/serde_json/fn.from_reader.html
    #[cfg(feature = "json")]
    #[cfg_attr(docsrs, doc(cfg(feature = "json")))]
    pub async fn json<T: DeserializeOwned>(self) -> crate::Result<T> {
        let full = self.bytes().await?;

        serde_json::from_slice(&full).map_err(crate::error::decode)
    }

    /// Get the full response body as `Bytes`.
    ///
    /// # Example
    ///
    /// ```
    /// # async fn run() -> Result<(), Box<dyn std::error::Error>> {
    /// let bytes = reqwest::get("http://httpbin.org/ip")
    ///     .await?
    ///     .bytes()
    ///     .await?;
    ///
    /// println!("bytes: {:?}", bytes);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn bytes(mut self) -> crate::Result<Bytes> {
        match &self.response_config.lowest_allowed_speed {
            None => hyper::body::to_bytes(self.res.into_body()).await,
            Some(lowest_speed_config) => {
                let mut window_start_time = Instant::now();
                let mut bytes_recieved_during_window = 0;
                // HELP: With capacity?
                let mut bytes_buffer = Vec::new();

                while let Some(bytes) = self.res.body_mut().try_next().await? {
                    let current_time = Instant::now();

                    // NOTE:
                    // Elapsed seconds conversion calculation is required to be done outside of
                    // if statement. `.as_secs()` floors elapsed, which may invalidate the inner
                    // assumption that elapsed > duration_window.
                    let elapsed_seconds = (current_time - window_start_time).as_secs();
                    let duriation_window_seconds = lowest_speed_config.duration_window.as_secs();

                    if elapsed_seconds >= duriation_window_seconds {
                        /*
                            * Protection against division by zero is guranteed by requiring
                            `duration_window` to be greater or equal to one second. `elapsed`
                            will in other words always be > 1 second whenever this branch
                            is executed.

                            * Using '<=' over '<' is required to handle the case when
                            lowest_speed_config.bytes_per_second is set to 0.

                            * Using integers removes the need to protect against NaN, +-Infinity
                            edge-cases, normally at the cost of precision. False positives
                            should, however, still not be able to occur given that the
                            if statement can equivalently be expressed as:

                            "abort if bytes_recieved_during_window <= min_bytes_per_per_window"

                            This equivalence leaves no room for rounding errors so long as
                            my math remains correct:

                            bytes_recieved_during_window / elapsed <= min_bytes_per_second <==>
                            bytes_recieved_during_window / elapsed <= min_bytes_per_window / duration_window

                            The surrounding `elapsed >= duration_window` if condition,
                            implies that the above inequality can be simplied to:

                            bytes_recieved_during_window <= min_bytes_per_window (Q.E.D)
                        */
                        if bytes_recieved_during_window / (elapsed_seconds as usize)
                            < lowest_speed_config.bytes_per_second
                        {
                            return Err(crate::error::body(
                                "body retrieval speed lower that configured limit, aborting",
                            ));
                        } else {
                            window_start_time = current_time;
                            bytes_recieved_during_window = 0;
                        }
                    } else {
                        bytes_recieved_during_window += bytes.len();
                    }

                    bytes_buffer.push(bytes)
                }

                Ok(bytes_buffer.concat().into())
            }
        }
    }

    /// Stream a chunk of the response body.
    ///
    /// When the response body has been exhausted, this will return `None`.
    ///
    /// # Example
    ///
    /// ```
    /// # async fn run() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut res = reqwest::get("https://hyper.rs").await?;
    ///
    /// while let Some(chunk) = res.chunk().await? {
    ///     println!("Chunk: {:?}", chunk);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn chunk(&mut self) -> crate::Result<Option<Bytes>> {
        if let Some(item) = self.res.body_mut().next().await {
            Ok(Some(item?))
        } else {
            Ok(None)
        }
    }

    /// Convert the response into a `Stream` of `Bytes` from the body.
    ///
    /// # Example
    ///
    /// ```
    /// use futures_util::StreamExt;
    ///
    /// # async fn run() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut stream = reqwest::get("http://httpbin.org/ip")
    ///     .await?
    ///     .bytes_stream();
    ///
    /// while let Some(item) = stream.next().await {
    ///     println!("Chunk: {:?}", item?);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Optional
    ///
    /// This requires the optional `stream` feature to be enabled.
    #[cfg(feature = "stream")]
    #[cfg_attr(docsrs, doc(cfg(feature = "stream")))]
    pub fn bytes_stream(self) -> impl futures_core::Stream<Item = crate::Result<Bytes>> {
        self.res.into_body()
    }

    // util methods

    /// Turn a response into an error if the server returned an error.
    ///
    /// # Example
    ///
    /// ```
    /// # use reqwest::Response;
    /// fn on_response(res: Response) {
    ///     match res.error_for_status() {
    ///         Ok(_res) => (),
    ///         Err(err) => {
    ///             // asserting a 400 as an example
    ///             // it could be any status between 400...599
    ///             assert_eq!(
    ///                 err.status(),
    ///                 Some(reqwest::StatusCode::BAD_REQUEST)
    ///             );
    ///         }
    ///     }
    /// }
    /// # fn main() {}
    /// ```
    pub fn error_for_status(self) -> crate::Result<Self> {
        let status = self.status();
        if status.is_client_error() || status.is_server_error() {
            Err(crate::error::status_code(*self.url, status))
        } else {
            Ok(self)
        }
    }

    /// Turn a reference to a response into an error if the server returned an error.
    ///
    /// # Example
    ///
    /// ```
    /// # use reqwest::Response;
    /// fn on_response(res: &Response) {
    ///     match res.error_for_status_ref() {
    ///         Ok(_res) => (),
    ///         Err(err) => {
    ///             // asserting a 400 as an example
    ///             // it could be any status between 400...599
    ///             assert_eq!(
    ///                 err.status(),
    ///                 Some(reqwest::StatusCode::BAD_REQUEST)
    ///             );
    ///         }
    ///     }
    /// }
    /// # fn main() {}
    /// ```
    pub fn error_for_status_ref(&self) -> crate::Result<&Self> {
        let status = self.status();
        if status.is_client_error() || status.is_server_error() {
            Err(crate::error::status_code(*self.url.clone(), status))
        } else {
            Ok(self)
        }
    }

    // private

    // The Response's body is an implementation detail.
    // You no longer need to get a reference to it, there are async methods
    // on the `Response` itself.
    //
    // This method is just used by the blocking API.
    #[cfg(feature = "blocking")]
    pub(crate) fn body_mut(&mut self) -> &mut Decoder {
        self.res.body_mut()
    }
}

impl fmt::Debug for Response {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Response")
            .field("url", self.url())
            .field("status", &self.status())
            .field("headers", self.headers())
            .finish()
    }
}

impl<T: Into<Body>> From<http::Response<T>> for Response {
    fn from(r: http::Response<T>) -> Response {
        let (mut parts, body) = r.into_parts();
        let body = body.into();
        let decoder = Decoder::detect(&mut parts.headers, body, Accepts::none());
        let url = parts
            .extensions
            .remove::<ResponseUrl>()
            .unwrap_or_else(|| ResponseUrl(Url::parse("http://no.url.provided.local").unwrap()));
        let url = url.0;
        let res = hyper::Response::from_parts(parts, decoder);
        Response {
            res,
            url: Box::new(url),
            response_config: Default::default(),
        }
    }
}

/// A `Response` can be piped as the `Body` of another request.
impl From<Response> for Body {
    fn from(r: Response) -> Body {
        Body::stream(r.res.into_body())
    }
}

#[cfg(test)]
mod tests {
    use std::{convert::From, task::Poll, time::Duration};

    use http::response::Builder;
    use url::Url;

    use crate::{
        async_impl::{
            body::Body,
            response::{ResponseConfig, SpeedLimitConfig},
        },
        Response, ResponseBuilderExt,
    };

    fn mock_url() -> Url {
        Url::parse("http://example.com").unwrap()
    }

    fn mock_response<T>(body: T) -> Response
    where
        Body: From<T>,
    {
        Response::from(
            Builder::new()
                .status(200)
                .url(mock_url())
                .body(body)
                .unwrap(),
        )
    }

    #[test]
    fn test_from_http_response() {
        let response = mock_response("foo");
        assert_eq!(response.status(), 200);
        assert_eq!(*response.url(), mock_url());
    }

    #[test]
    fn disallows_subsecond_duration_windows() {
        assert!(SpeedLimitConfig::try_new(10, Duration::from_millis(999)).is_err())
    }

    #[tokio::test]
    async fn gets_bytes_over_lowest_allowed_speed() {
        verify_lower_speed_limit(LowerLimit::Above).await
    }

    #[tokio::test]
    async fn error_under_lowest_allowed_speed() {
        verify_lower_speed_limit(LowerLimit::Below).await
    }

    enum LowerLimit {
        Below,
        Above,
    }

    async fn verify_lower_speed_limit(threshold: LowerLimit) {
        const DURATION_WINDOW: Duration = Duration::from_secs(2);
        const SENT_DATA: [u8; 2] = [0; 2];

        let response_config = ResponseConfig {
            lowest_allowed_speed: Some(SpeedLimitConfig {
                bytes_per_second: 10,
                duration_window: DURATION_WINDOW,
            }),
        };

        let (mut sender, body) = hyper::Body::channel();
        let mut response = mock_response(body);
        response.response_config = response_config.clone();

        let mut bytes_future = Box::pin(response.bytes());
        assert!(matches!(
            futures_util::poll!(&mut bytes_future),
            Poll::Pending
        ));

        tokio::time::sleep(match threshold {
            LowerLimit::Below => DURATION_WINDOW + Duration::from_secs(1),
            LowerLimit::Above => DURATION_WINDOW - Duration::from_secs(1),
        })
        .await;

        sender.send_data(SENT_DATA.as_slice().into()).await.unwrap();
        drop(sender);

        let bytes_result = bytes_future.await;
        match threshold {
            LowerLimit::Below => assert!(bytes_result.is_err()),
            LowerLimit::Above => assert_eq!(SENT_DATA.as_slice(), bytes_result.unwrap()),
        }
    }
}
