use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

#[cfg(feature = "gzip")]
use async_compression::tokio::bufread::GzipDecoder;

#[cfg(feature = "brotli")]
use async_compression::tokio::bufread::BrotliDecoder;

#[cfg(feature = "zstd")]
use async_compression::tokio::bufread::ZstdDecoder;

#[cfg(feature = "deflate")]
use async_compression::tokio::bufread::ZlibDecoder;

use bytes::Bytes;
use futures_core::Stream;
use futures_util::stream::Peekable;
use http::HeaderMap;
use hyper::body::Body as HttpBody;
use hyper::body::Frame;

#[cfg(any(
    feature = "gzip",
    feature = "brotli",
    feature = "zstd",
    feature = "deflate"
))]
use tokio_util::codec::{BytesCodec, FramedRead};
#[cfg(any(
    feature = "gzip",
    feature = "brotli",
    feature = "zstd",
    feature = "deflate"
))]
use tokio_util::io::StreamReader;

use super::body::ResponseBody;
use crate::error;

#[derive(Clone, Copy, Debug)]
pub(super) struct Accepts {
    #[cfg(feature = "gzip")]
    pub(super) gzip: bool,
    #[cfg(feature = "brotli")]
    pub(super) brotli: bool,
    #[cfg(feature = "zstd")]
    pub(super) zstd: bool,
    #[cfg(feature = "deflate")]
    pub(super) deflate: bool,
}

impl Accepts {
    pub fn none() -> Self {
        Self {
            #[cfg(feature = "gzip")]
            gzip: false,
            #[cfg(feature = "brotli")]
            brotli: false,
            #[cfg(feature = "zstd")]
            zstd: false,
            #[cfg(feature = "deflate")]
            deflate: false,
        }
    }
}

/// A response decompressor over a non-blocking stream of chunks.
///
/// The inner decoder may be constructed asynchronously.
pub(crate) struct Decoder {
    inner: Inner,
}

type PeekableIoStream = Peekable<IoStream>;

#[cfg(any(
    feature = "gzip",
    feature = "zstd",
    feature = "brotli",
    feature = "deflate"
))]
type PeekableIoStreamReader = StreamReader<PeekableIoStream, Bytes>;

enum Inner {
    /// A `PlainText` decoder just returns the response content as is.
    PlainText(ResponseBody),

    /// A `Gzip` decoder will uncompress the gzipped response content before returning it.
    #[cfg(feature = "gzip")]
    Gzip(Pin<Box<FramedRead<GzipDecoder<PeekableIoStreamReader>, BytesCodec>>>),

    /// A `Brotli` decoder will uncompress the brotlied response content before returning it.
    #[cfg(feature = "brotli")]
    Brotli(Pin<Box<FramedRead<BrotliDecoder<PeekableIoStreamReader>, BytesCodec>>>),

    /// A `Zstd` decoder will uncompress the zstd compressed response content before returning it.
    #[cfg(feature = "zstd")]
    Zstd(Pin<Box<FramedRead<ZstdDecoder<PeekableIoStreamReader>, BytesCodec>>>),

    /// A `Deflate` decoder will uncompress the deflated response content before returning it.
    #[cfg(feature = "deflate")]
    Deflate(Pin<Box<FramedRead<ZlibDecoder<PeekableIoStreamReader>, BytesCodec>>>),

    /// A decoder that doesn't have a value yet.
    #[cfg(any(
        feature = "brotli",
        feature = "zstd",
        feature = "gzip",
        feature = "deflate"
    ))]
    Pending(Pin<Box<Pending>>),
}

/// A future attempt to poll the response body for EOF so we know whether to use gzip or not.
struct Pending(PeekableIoStream, DecoderType);

pub(crate) struct IoStream<B = ResponseBody>(B);

enum DecoderType {
    #[cfg(feature = "gzip")]
    Gzip,
    #[cfg(feature = "brotli")]
    Brotli,
    #[cfg(feature = "zstd")]
    Zstd,
    #[cfg(feature = "deflate")]
    Deflate,
}

impl fmt::Debug for Decoder {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Decoder").finish()
    }
}

impl Decoder {
    #[cfg(feature = "blocking")]
    pub(crate) fn empty() -> Decoder {
        Decoder {
            inner: Inner::PlainText(empty()),
        }
    }

    #[cfg(feature = "blocking")]
    pub(crate) fn into_stream(self) -> IoStream<Self> {
        IoStream(self)
    }

    /// A plain text decoder.
    ///
    /// This decoder will emit the underlying chunks as-is.
    fn plain_text(body: ResponseBody) -> Decoder {
        Decoder {
            inner: Inner::PlainText(body),
        }
    }

    /// A gzip decoder.
    ///
    /// This decoder will buffer and decompress chunks that are gzipped.
    #[cfg(feature = "gzip")]
    fn gzip(body: ResponseBody) -> Decoder {
        use futures_util::StreamExt;

        Decoder {
            inner: Inner::Pending(Box::pin(Pending(
                IoStream(body).peekable(),
                DecoderType::Gzip,
            ))),
        }
    }

    /// A brotli decoder.
    ///
    /// This decoder will buffer and decompress chunks that are brotlied.
    #[cfg(feature = "brotli")]
    fn brotli(body: ResponseBody) -> Decoder {
        use futures_util::StreamExt;

        Decoder {
            inner: Inner::Pending(Box::pin(Pending(
                IoStream(body).peekable(),
                DecoderType::Brotli,
            ))),
        }
    }

    /// A zstd decoder.
    ///
    /// This decoder will buffer and decompress chunks that are zstd compressed.
    #[cfg(feature = "zstd")]
    fn zstd(body: ResponseBody) -> Decoder {
        use futures_util::StreamExt;

        Decoder {
            inner: Inner::Pending(Box::pin(Pending(
                IoStream(body).peekable(),
                DecoderType::Zstd,
            ))),
        }
    }

    /// A deflate decoder.
    ///
    /// This decoder will buffer and decompress chunks that are deflated.
    #[cfg(feature = "deflate")]
    fn deflate(body: ResponseBody) -> Decoder {
        use futures_util::StreamExt;

        Decoder {
            inner: Inner::Pending(Box::pin(Pending(
                IoStream(body).peekable(),
                DecoderType::Deflate,
            ))),
        }
    }

    #[cfg(any(
        feature = "brotli",
        feature = "zstd",
        feature = "gzip",
        feature = "deflate"
    ))]
    fn detect_encoding(headers: &mut HeaderMap, encoding_str: &str) -> bool {
        use http::header::{CONTENT_ENCODING, CONTENT_LENGTH, TRANSFER_ENCODING};
        use log::warn;

        let mut is_content_encoded = {
            headers
                .get_all(CONTENT_ENCODING)
                .iter()
                .any(|enc| enc == encoding_str)
                || headers
                    .get_all(TRANSFER_ENCODING)
                    .iter()
                    .any(|enc| enc == encoding_str)
        };
        if is_content_encoded {
            if let Some(content_length) = headers.get(CONTENT_LENGTH) {
                if content_length == "0" {
                    warn!("{encoding_str} response with content-length of 0");
                    is_content_encoded = false;
                }
            }
        }
        if is_content_encoded {
            headers.remove(CONTENT_ENCODING);
            headers.remove(CONTENT_LENGTH);
        }
        is_content_encoded
    }

    /// Constructs a Decoder from a hyper request.
    ///
    /// A decoder is just a wrapper around the hyper request that knows
    /// how to decode the content body of the request.
    ///
    /// Uses the correct variant by inspecting the Content-Encoding header.
    pub(super) fn detect(
        _headers: &mut HeaderMap,
        body: ResponseBody,
        _accepts: Accepts,
    ) -> Decoder {
        #[cfg(feature = "gzip")]
        {
            if _accepts.gzip && Decoder::detect_encoding(_headers, "gzip") {
                return Decoder::gzip(body);
            }
        }

        #[cfg(feature = "brotli")]
        {
            if _accepts.brotli && Decoder::detect_encoding(_headers, "br") {
                return Decoder::brotli(body);
            }
        }

        #[cfg(feature = "zstd")]
        {
            if _accepts.zstd && Decoder::detect_encoding(_headers, "zstd") {
                return Decoder::zstd(body);
            }
        }

        #[cfg(feature = "deflate")]
        {
            if _accepts.deflate && Decoder::detect_encoding(_headers, "deflate") {
                return Decoder::deflate(body);
            }
        }

        Decoder::plain_text(body)
    }
}

impl HttpBody for Decoder {
    type Data = Bytes;
    type Error = crate::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        match self.inner {
            #[cfg(any(
                feature = "brotli",
                feature = "zstd",
                feature = "gzip",
                feature = "deflate"
            ))]
            Inner::Pending(ref mut future) => match Pin::new(future).poll(cx) {
                Poll::Ready(Ok(inner)) => {
                    self.inner = inner;
                    self.poll_frame(cx)
                }
                Poll::Ready(Err(e)) => Poll::Ready(Some(Err(crate::error::decode_io(e)))),
                Poll::Pending => Poll::Pending,
            },
            Inner::PlainText(ref mut body) => {
                match futures_core::ready!(Pin::new(body).poll_frame(cx)) {
                    Some(Ok(frame)) => Poll::Ready(Some(Ok(frame))),
                    Some(Err(err)) => Poll::Ready(Some(Err(crate::error::decode(err)))),
                    None => Poll::Ready(None),
                }
            }
            #[cfg(feature = "gzip")]
            Inner::Gzip(ref mut decoder) => {
                match futures_core::ready!(Pin::new(decoder).poll_next(cx)) {
                    Some(Ok(bytes)) => Poll::Ready(Some(Ok(Frame::data(bytes.freeze())))),
                    Some(Err(err)) => Poll::Ready(Some(Err(crate::error::decode_io(err)))),
                    None => Poll::Ready(None),
                }
            }
            #[cfg(feature = "brotli")]
            Inner::Brotli(ref mut decoder) => {
                match futures_core::ready!(Pin::new(decoder).poll_next(cx)) {
                    Some(Ok(bytes)) => Poll::Ready(Some(Ok(Frame::data(bytes.freeze())))),
                    Some(Err(err)) => Poll::Ready(Some(Err(crate::error::decode_io(err)))),
                    None => Poll::Ready(None),
                }
            }
            #[cfg(feature = "zstd")]
            Inner::Zstd(ref mut decoder) => {
                match futures_core::ready!(Pin::new(decoder).poll_next(cx)) {
                    Some(Ok(bytes)) => Poll::Ready(Some(Ok(Frame::data(bytes.freeze())))),
                    Some(Err(err)) => Poll::Ready(Some(Err(crate::error::decode_io(err)))),
                    None => Poll::Ready(None),
                }
            }
            #[cfg(feature = "deflate")]
            Inner::Deflate(ref mut decoder) => {
                match futures_core::ready!(Pin::new(decoder).poll_next(cx)) {
                    Some(Ok(bytes)) => Poll::Ready(Some(Ok(Frame::data(bytes.freeze())))),
                    Some(Err(err)) => Poll::Ready(Some(Err(crate::error::decode_io(err)))),
                    None => Poll::Ready(None),
                }
            }
        }
    }

    fn size_hint(&self) -> http_body::SizeHint {
        match self.inner {
            Inner::PlainText(ref body) => HttpBody::size_hint(body),
            // the rest are "unknown", so default
            #[cfg(any(
                feature = "brotli",
                feature = "zstd",
                feature = "gzip",
                feature = "deflate"
            ))]
            _ => http_body::SizeHint::default(),
        }
    }
}

fn empty() -> ResponseBody {
    use http_body_util::{combinators::BoxBody, BodyExt, Empty};
    BoxBody::new(Empty::new().map_err(|never| match never {}))
}

impl Future for Pending {
    type Output = Result<Inner, std::io::Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        use futures_util::StreamExt;

        match futures_core::ready!(Pin::new(&mut self.0).poll_peek(cx)) {
            Some(Ok(_)) => {
                // fallthrough
            }
            Some(Err(_e)) => {
                // error was just a ref, so we need to really poll to move it
                return Poll::Ready(Err(futures_core::ready!(
                    Pin::new(&mut self.0).poll_next(cx)
                )
                .expect("just peeked Some")
                .unwrap_err()));
            }
            None => return Poll::Ready(Ok(Inner::PlainText(empty()))),
        };

        let _body = std::mem::replace(&mut self.0, IoStream(empty()).peekable());

        match self.1 {
            #[cfg(feature = "brotli")]
            DecoderType::Brotli => Poll::Ready(Ok(Inner::Brotli(Box::pin(FramedRead::new(
                BrotliDecoder::new(StreamReader::new(_body)),
                BytesCodec::new(),
            ))))),
            #[cfg(feature = "zstd")]
            DecoderType::Zstd => Poll::Ready(Ok(Inner::Zstd(Box::pin(FramedRead::new(
                ZstdDecoder::new(StreamReader::new(_body)),
                BytesCodec::new(),
            ))))),
            #[cfg(feature = "gzip")]
            DecoderType::Gzip => Poll::Ready(Ok(Inner::Gzip(Box::pin(FramedRead::new(
                GzipDecoder::new(StreamReader::new(_body)),
                BytesCodec::new(),
            ))))),
            #[cfg(feature = "deflate")]
            DecoderType::Deflate => Poll::Ready(Ok(Inner::Deflate(Box::pin(FramedRead::new(
                ZlibDecoder::new(StreamReader::new(_body)),
                BytesCodec::new(),
            ))))),
        }
    }
}

impl<B> Stream for IoStream<B>
where
    B: HttpBody<Data = Bytes> + Unpin,
    B::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    type Item = Result<Bytes, std::io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        loop {
            return match futures_core::ready!(Pin::new(&mut self.0).poll_frame(cx)) {
                Some(Ok(frame)) => {
                    // skip non-data frames
                    if let Ok(buf) = frame.into_data() {
                        Poll::Ready(Some(Ok(buf)))
                    } else {
                        continue;
                    }
                }
                Some(Err(err)) => Poll::Ready(Some(Err(error::into_io(err.into())))),
                None => Poll::Ready(None),
            };
        }
    }
}

// ===== impl Accepts =====

impl Accepts {
    /*
    pub(super) fn none() -> Self {
        Accepts {
            #[cfg(feature = "gzip")]
            gzip: false,
            #[cfg(feature = "brotli")]
            brotli: false,
            #[cfg(feature = "zstd")]
            zstd: false,
            #[cfg(feature = "deflate")]
            deflate: false,
        }
    }
    */

    pub(super) fn as_str(&self) -> Option<&'static str> {
        match (
            self.is_gzip(),
            self.is_brotli(),
            self.is_zstd(),
            self.is_deflate(),
        ) {
            (true, true, true, true) => Some("gzip, br, zstd, deflate"),
            (true, true, false, true) => Some("gzip, br, deflate"),
            (true, true, true, false) => Some("gzip, br, zstd"),
            (true, true, false, false) => Some("gzip, br"),
            (true, false, true, true) => Some("gzip, zstd, deflate"),
            (true, false, false, true) => Some("gzip, zstd, deflate"),
            (false, true, true, true) => Some("br, zstd, deflate"),
            (false, true, false, true) => Some("br, zstd, deflate"),
            (true, false, true, false) => Some("gzip, zstd"),
            (true, false, false, false) => Some("gzip"),
            (false, true, true, false) => Some("br, zstd"),
            (false, true, false, false) => Some("br"),
            (false, false, true, true) => Some("zstd, deflate"),
            (false, false, true, false) => Some("zstd"),
            (false, false, false, true) => Some("deflate"),
            (false, false, false, false) => None,
        }
    }

    fn is_gzip(&self) -> bool {
        #[cfg(feature = "gzip")]
        {
            self.gzip
        }

        #[cfg(not(feature = "gzip"))]
        {
            false
        }
    }

    fn is_brotli(&self) -> bool {
        #[cfg(feature = "brotli")]
        {
            self.brotli
        }

        #[cfg(not(feature = "brotli"))]
        {
            false
        }
    }

    fn is_zstd(&self) -> bool {
        #[cfg(feature = "zstd")]
        {
            self.zstd
        }

        #[cfg(not(feature = "zstd"))]
        {
            false
        }
    }

    fn is_deflate(&self) -> bool {
        #[cfg(feature = "deflate")]
        {
            self.deflate
        }

        #[cfg(not(feature = "deflate"))]
        {
            false
        }
    }
}

impl Default for Accepts {
    fn default() -> Accepts {
        Accepts {
            #[cfg(feature = "gzip")]
            gzip: true,
            #[cfg(feature = "brotli")]
            brotli: true,
            #[cfg(feature = "zstd")]
            zstd: true,
            #[cfg(feature = "deflate")]
            deflate: true,
        }
    }
}
