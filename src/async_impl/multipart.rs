//! multipart/form-data
use std::borrow::Cow;
use std::fmt;

use mime_guess::Mime;
use url::percent_encoding::{self, EncodeSet, PATH_SEGMENT_ENCODE_SET};
use uuid::Uuid;
use http::HeaderMap;

use futures::Stream;

use super::Body;

/// An async multipart/form-data request.
pub struct Form {
    boundary: String,
    fields: Vec<(Cow<'static, str>, Part)>,
    percent_encoding: PercentEncoding,
}

enum PercentEncoding {
    PathSegment,
    AttrChar,
}

impl Form {
    /// Creates a new async Form without any content.
    pub fn new() -> Form {
        Form {
            boundary: format!("{}", Uuid::new_v4().to_simple()),
            fields: Vec::new(),
            percent_encoding: PercentEncoding::PathSegment,
        }
    }

    /// Get the boundary that this form will use.
    #[inline]
    pub fn boundary(&self) -> &str {
        &self.boundary
    }

    /// Add a data field with supplied name and value.
    ///
    /// # Examples
    ///
    /// ```
    /// let form = reqwest::async::multipart::Form::new()
    ///     .text("username", "seanmonstar")
    ///     .text("password", "secret");
    /// ```
    pub fn text<T, U>(self, name: T, value: U) -> Form
    where T: Into<Cow<'static, str>>,
          U: Into<Cow<'static, str>>,
    {
        self.part(name, Part::text(value))
    }

    /// Adds a customized Part.
    pub fn part<T>(mut self, name: T, part: Part) -> Form
    where T: Into<Cow<'static, str>>,
    {
        self.fields.push((name.into(), part));
        self
    }

    /// Configure this `Form` to percent-encode using the `path-segment` rules.
    pub fn percent_encode_path_segment(mut self) -> Form {
        self.percent_encoding = PercentEncoding::PathSegment;
        self
    }

    /// Configure this `Form` to percent-encode using the `attr-char` rules.
    pub fn percent_encode_attr_chars(mut self) -> Form {
        self.percent_encoding = PercentEncoding::AttrChar;
        self
    }

    /// Consume this instance and transform into an instance of hyper::Body for use in a request.
    pub(crate) fn stream(mut self) -> hyper::Body {
      if self.fields.len() == 0 {
        return hyper::Body::empty();
      }

      // create initial part to init reduce chain
      let (name, part) = self.fields.remove(0);
      let start = self.part_stream(name, part);

      let fields = self.take_fields();
      // for each field, chain an additional stream
      let stream = fields.into_iter().fold(start, |memo, (name, part)| {
        let part_stream = self.part_stream(name, part);
        hyper::Body::wrap_stream(memo.chain(part_stream))
      });
      // append special ending boundary
      let last = hyper::Body::from(format!("--{}--\r\n", self.boundary));
      hyper::Body::wrap_stream(stream.chain(last))
    }

    /// Generate a hyper::Body stream for a single Part instance of a Form request. 
    pub fn part_stream<T: Into<Cow<'static, str>>>(&mut self, name: T, part: Part) -> hyper::Body {
      // start with boundary
      let boundary = hyper::Body::from(format!("--{}\r\n", self.boundary));
      // append headers
      let header = hyper::Body::from({
        let mut h = self.percent_encoding.encode_headers(&name.into(), &part);
        h.extend_from_slice(b"\r\n\r\n");
        h
      });
      // then append form data followed by terminating CRLF
      hyper::Body::wrap_stream(boundary.chain(header).chain(hyper::Body::wrap_stream(part.value)).chain(hyper::Body::from("\r\n".to_owned())))
    }

    /// Take the fields vector of this instance, replacing with an empty vector.
    fn take_fields(&mut self) -> Vec<(Cow<'static, str>, Part)> {
      std::mem::replace(&mut self.fields, Vec::new())
    }
}

impl fmt::Debug for Form {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Form")
            .field("boundary", &self.boundary)
            .field("parts", &self.fields)
            .finish()
    }
}


/// A field in a multipart form.
pub struct Part {
    value: Body,
    mime: Option<Mime>,
    file_name: Option<Cow<'static, str>>,
    headers: HeaderMap,
}

impl Part {
    /// Makes a text parameter.
    pub fn text<T>(value: T) -> Part
    where T: Into<Cow<'static, str>>,
    {
        let body = match value.into() {
            Cow::Borrowed(slice) => Body::from(slice),
            Cow::Owned(string) => Body::from(string),
        };
        Part::new(body)
    }

    /// Makes a new parameter from arbitrary bytes
    pub fn bytes<T>(value: T) -> Part
    where T: Into<Cow<'static, [u8]>>
    {
        let body = match value.into() {
            Cow::Borrowed(slice) => Body::from(slice),
            Cow::Owned(vec) => Body::from(vec),
        };
        Part::new(body)
    }

    /// Makes a new parameter from an arbitrary stream.
    pub fn stream<T: Stream + Send + 'static>(value: T) -> Part
    where hyper::Chunk: std::convert::From<<T as futures::Stream>::Item>,
          <T as futures::Stream>::Error: std::error::Error + Send + Sync {
      Part::new(Body::wrap(hyper::Body::wrap_stream(value)))
    }

    fn new(value: Body) -> Part {
        Part {
            value: value,
            mime: None,
            file_name: None,
            headers: HeaderMap::default()
        }
    }

    /// Tries to set the mime of this part.
    pub fn mime_str(mut self, mime: &str) -> ::Result<Part> {
        self.mime = Some(try_!(mime.parse()));
        Ok(self)
    }

    // Re-enable when mime 0.4 is available, with split MediaType/MediaRange.
    #[cfg(test)]
    fn mime(mut self, mime: Mime) -> Part {
        self.mime = Some(mime);
        self
    }

    /// Sets the filename, builder style.
    pub fn file_name<T: Into<Cow<'static, str>>>(mut self, filename: T) -> Part {
        self.file_name = Some(filename.into());
        self
    }

    /// Returns a reference to the map with additional header fields
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// Returns a reference to the map with additional header fields
    pub fn headers_mut(&mut self) -> &mut HeaderMap {
        &mut self.headers
    }
}

impl fmt::Debug for Part {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Part")
            .field("value", &self.value)
            .field("mime", &self.mime)
            .field("file_name", &self.file_name)
            .field("headers", &self.headers)
            .finish()
    }
}

#[derive(Debug, Clone)]
struct AttrCharEncodeSet;

impl EncodeSet for AttrCharEncodeSet {
    fn contains(&self, ch: u8) -> bool {
        match ch as char {
             '!'  => false,
             '#'  => false,
             '$'  => false,
             '&'  => false,
             '+'  => false,
             '-'  => false,
             '.' => false,
             '^'  => false,
             '_'  => false,
             '`'  => false,
             '|'  => false,
             '~' => false,
              _ => {
                  let is_alpha_numeric = ch >= 0x41 && ch <= 0x5a || ch >= 0x61 && ch <= 0x7a || ch >= 0x30 && ch <= 0x39;
                  !is_alpha_numeric
              }
        }
    }

}

impl PercentEncoding {
    fn encode_headers(&self, name: &str, field: &Part) -> Vec<u8> {
        let s = format!(
            "Content-Disposition: form-data; {}{}{}",
            self.format_parameter("name", name),
            match field.file_name {
                Some(ref file_name) => format!("; {}", self.format_parameter("filename", file_name)),
                None => String::new(),
            },
            match field.mime {
                Some(ref mime) => format!("\r\nContent-Type: {}", mime),
                None => "".to_string(),
            },
        );
        field.headers.iter().fold(s.into_bytes(), |mut header, (k,v)| {
            header.extend_from_slice(b"\r\n");
            header.extend_from_slice(k.as_str().as_bytes());
            header.extend_from_slice(b": ");
            header.extend_from_slice(v.as_bytes());
            header
        })
    }

    fn format_parameter(&self, name: &str, value: &str) -> String {
        let legal_value = match *self {
            PercentEncoding::PathSegment => {
                percent_encoding::utf8_percent_encode(value, PATH_SEGMENT_ENCODE_SET)
                    .to_string()
            },
            PercentEncoding::AttrChar => {
                percent_encoding::utf8_percent_encode(value, AttrCharEncodeSet)
                    .to_string()
            },
        };
        if value.len() == legal_value.len() {
            // nothing has been percent encoded
            format!("{}=\"{}\"", name, value)
        } else {
            // something has been percent encoded
            format!("{}*=utf-8''{}", name, legal_value)
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use tokio;

    #[test]
    fn form_empty() {
        let form = Form::new();

        let mut rt = tokio::runtime::current_thread::Runtime::new().expect("new rt");
        let body_ft = form.stream();

        let out = rt.block_on(body_ft.map(|c| c.into_bytes()).concat2());
        assert_eq!(out.unwrap(), Vec::new());
    }

    #[test]
    fn stream_to_end() {
        let mut form = Form::new()
            .part("reader1", Part::stream(futures::stream::once::<_, hyper::Error>(Ok(hyper::Chunk::from("part1".to_owned())))))
            .part("key1", Part::text("value1"))
            .part(
                "key2",
                Part::text("value2").mime(::mime::IMAGE_BMP),
            )
            .part("reader2", Part::stream(futures::stream::once::<_, hyper::Error>(Ok(hyper::Chunk::from("part2".to_owned())))))
            .part(
                "key3",
                Part::text("value3").file_name("filename"),
            );
        form.boundary = "boundary".to_string();
        let expected = "--boundary\r\n\
                        Content-Disposition: form-data; name=\"reader1\"\r\n\r\n\
                        part1\r\n\
                        --boundary\r\n\
                        Content-Disposition: form-data; name=\"key1\"\r\n\r\n\
                        value1\r\n\
                        --boundary\r\n\
                        Content-Disposition: form-data; name=\"key2\"\r\n\
                        Content-Type: image/bmp\r\n\r\n\
                        value2\r\n\
                        --boundary\r\n\
                        Content-Disposition: form-data; name=\"reader2\"\r\n\r\n\
                        part2\r\n\
                        --boundary\r\n\
                        Content-Disposition: form-data; name=\"key3\"; filename=\"filename\"\r\n\r\n\
                        value3\r\n--boundary--\r\n";
        let mut rt = tokio::runtime::current_thread::Runtime::new().expect("new rt");
        let body_ft = form.stream();

        let out = rt.block_on(body_ft.map(|c| c.into_bytes()).concat2()).unwrap();
        // These prints are for debug purposes in case the test fails
        println!(
            "START REAL\n{}\nEND REAL",
            ::std::str::from_utf8(&out).unwrap()
        );
        println!("START EXPECTED\n{}\nEND EXPECTED", expected);
        assert_eq!(::std::str::from_utf8(&out).unwrap(), expected);
    }

    #[test]
    fn stream_to_end_with_header() {
        let mut part = Part::text("value2").mime(::mime::IMAGE_BMP);
        part.headers_mut().insert("Hdr3", "/a/b/c".parse().unwrap());
        let mut form = Form::new().part("key2", part);
        form.boundary = "boundary".to_string();
        let expected = "--boundary\r\n\
                        Content-Disposition: form-data; name=\"key2\"\r\n\
                        Content-Type: image/bmp\r\n\
                        hdr3: /a/b/c\r\n\
                        \r\n\
                        value2\r\n\
                        --boundary--\r\n";
        let mut rt = tokio::runtime::current_thread::Runtime::new().expect("new rt");
        let body_ft = form.stream();

        let out = rt.block_on(body_ft.map(|c| c.into_bytes()).concat2()).unwrap();
        // These prints are for debug purposes in case the test fails
        println!(
            "START REAL\n{}\nEND REAL",
            ::std::str::from_utf8(&out).unwrap()
        );
        println!("START EXPECTED\n{}\nEND EXPECTED", expected);
        assert_eq!(::std::str::from_utf8(&out).unwrap(), expected);
    }

    #[test]
    fn header_percent_encoding() {
        let name = "start%'\"\r\nßend";
        let field = Part::text("");

        assert_eq!(
            PercentEncoding::PathSegment.encode_headers(name, &field),
            &b"Content-Disposition: form-data; name*=utf-8''start%25'%22%0D%0A%C3%9Fend"[..]
        );

        assert_eq!(
            PercentEncoding::AttrChar.encode_headers(name, &field),
            &b"Content-Disposition: form-data; name*=utf-8''start%25%27%22%0D%0A%C3%9Fend"[..]
        );
    }
}
