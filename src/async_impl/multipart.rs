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
    inner: FormParts<Part>,
}

/// A field in a multipart form.
pub struct Part {
    meta: PartMetadata,
    value: Body,
}

pub(crate) struct FormParts<P> {
    pub(crate) boundary: String,
    pub(crate) computed_headers: Vec<Vec<u8>>,
    pub(crate) fields: Vec<(Cow<'static, str>, P)>,
    pub(crate) percent_encoding: PercentEncoding,
}

pub(crate) struct PartMetadata {
    mime: Option<Mime>,
    file_name: Option<Cow<'static, str>>,
    pub(crate) headers: HeaderMap,
}

pub(crate) trait PartProps {
    fn value_len(&self) -> Option<u64>;
    fn metadata(&self) -> &PartMetadata;
}

// ===== impl Form =====

impl Form {
    /// Creates a new async Form without any content.
    pub fn new() -> Form {
        Form {
            inner: FormParts::new(),
        }
    }

    /// Get the boundary that this form will use.
    #[inline]
    pub fn boundary(&self) -> &str {
        self.inner.boundary()
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
    where
        T: Into<Cow<'static, str>>,
        U: Into<Cow<'static, str>>,
    {
        self.part(name, Part::text(value))
    }

    /// Adds a customized Part.
    pub fn part<T>(self, name: T, part: Part) -> Form
    where
        T: Into<Cow<'static, str>>,
    {
        self.with_inner(move |inner| inner.part(name, part))
    }

    /// Configure this `Form` to percent-encode using the `path-segment` rules.
    pub fn percent_encode_path_segment(self) -> Form {
        self.with_inner(|inner| inner.percent_encode_path_segment())
    }

    /// Configure this `Form` to percent-encode using the `attr-char` rules.
    pub fn percent_encode_attr_chars(self) -> Form {
        self.with_inner(|inner| inner.percent_encode_attr_chars())
    }

    /// Configure this `Form` to skip percent-encoding
    pub fn percent_encode_noop(self) -> Form {
        self.with_inner(|inner| inner.percent_encode_noop())
    }

    /// Consume this instance and transform into an instance of hyper::Body for use in a request.
    pub(crate) fn stream(mut self) -> hyper::Body {
        if self.inner.fields.len() == 0 {
            return hyper::Body::empty();
        }

        // create initial part to init reduce chain
        let (name, part) = self.inner.fields.remove(0);
        let start = self.part_stream(name, part);

        let fields = self.inner.take_fields();
        // for each field, chain an additional stream
        let stream = fields.into_iter().fold(start, |memo, (name, part)| {
            let part_stream = self.part_stream(name, part);
            hyper::Body::wrap_stream(memo.chain(part_stream))
        });
        // append special ending boundary
        let last = hyper::Body::from(format!("--{}--\r\n", self.boundary()));
        hyper::Body::wrap_stream(stream.chain(last))
    }

    /// Generate a hyper::Body stream for a single Part instance of a Form request. 
    pub(crate) fn part_stream<T>(&mut self, name: T, part: Part) -> hyper::Body
    where
        T: Into<Cow<'static, str>>,
    {
        // start with boundary
        let boundary = hyper::Body::from(format!("--{}\r\n", self.boundary()));
        // append headers
        let header = hyper::Body::from({
            let mut h = self.inner.percent_encoding.encode_headers(&name.into(), &part.meta);
            h.extend_from_slice(b"\r\n\r\n");
            h
        });
        // then append form data followed by terminating CRLF
        hyper::Body::wrap_stream(boundary.chain(header).chain(hyper::Body::wrap_stream(part.value)).chain(hyper::Body::from("\r\n".to_owned())))
    }

    pub(crate) fn compute_length(&mut self) -> Option<u64> {
        self.inner.compute_length()
    }

    fn with_inner<F>(self, func: F) -> Self
    where
        F: FnOnce(FormParts<Part>) -> FormParts<Part>,
    {
        Form {
            inner: func(self.inner),
        }
    }
}

impl fmt::Debug for Form {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.inner.fmt_fields("Form", f)
    }
}

// ===== impl Part =====

impl Part {
    /// Makes a text parameter.
    pub fn text<T>(value: T) -> Part
    where
        T: Into<Cow<'static, str>>,
    {
        let body = match value.into() {
            Cow::Borrowed(slice) => Body::from(slice),
            Cow::Owned(string) => Body::from(string),
        };
        Part::new(body)
    }

    /// Makes a new parameter from arbitrary bytes.
    pub fn bytes<T>(value: T) -> Part
    where
        T: Into<Cow<'static, [u8]>>,
    {
        let body = match value.into() {
            Cow::Borrowed(slice) => Body::from(slice),
            Cow::Owned(vec) => Body::from(vec),
        };
        Part::new(body)
    }

    /// Makes a new parameter from an arbitrary stream.
    pub fn stream<T>(value: T) -> Part
    where
        T: Stream + Send + 'static,
        T::Error: std::error::Error + Send + Sync,
        hyper::Chunk: std::convert::From<T::Item>,
    {
        Part::new(Body::wrap(hyper::Body::wrap_stream(value)))
    }

    fn new(value: Body) -> Part {
        Part {
            meta: PartMetadata::new(),
            value,
        }
    }

    /// Tries to set the mime of this part.
    pub fn mime_str(self, mime: &str) -> ::Result<Part> {
        Ok(self.mime(try_!(mime.parse())))
    }

    // Re-export when mime 0.4 is available, with split MediaType/MediaRange.
    fn mime(self, mime: Mime) -> Part {
        self.with_inner(move |inner| inner.mime(mime))
    }

    /// Sets the filename, builder style.
    pub fn file_name<T>(self, filename: T) -> Part
    where
        T: Into<Cow<'static, str>>,
    {
        self.with_inner(move |inner| inner.file_name(filename))
    }

    fn with_inner<F>(self, func: F) -> Self
    where
        F: FnOnce(PartMetadata) -> PartMetadata,
    {
        Part {
            meta: func(self.meta),
            value: self.value,
        }
    }
}

impl fmt::Debug for Part {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut dbg = f.debug_struct("Part");
        dbg.field("value", &self.value);
        self.meta.fmt_fields(&mut dbg);
        dbg.finish()
    }
}

impl PartProps for Part {
    fn value_len(&self) -> Option<u64> {
        self.value.content_length()
    }

    fn metadata(&self) -> &PartMetadata {
        &self.meta
    }
}

// ===== impl FormParts =====

impl<P: PartProps> FormParts<P> {
    pub(crate) fn new() -> Self {
        FormParts {
            boundary: format!("{}", Uuid::new_v4().to_simple()),
            computed_headers: Vec::new(),
            fields: Vec::new(),
            percent_encoding: PercentEncoding::PathSegment,
        }
    }

    pub(crate) fn boundary(&self) -> &str {
        &self.boundary
    }

    /// Adds a customized Part.
    pub(crate) fn part<T>(mut self, name: T, part: P) -> Self
    where
        T: Into<Cow<'static, str>>,
    {
        self.fields.push((name.into(), part));
        self
    }

    /// Configure this `Form` to percent-encode using the `path-segment` rules.
    pub(crate) fn percent_encode_path_segment(mut self) -> Self {
        self.percent_encoding = PercentEncoding::PathSegment;
        self
    }

    /// Configure this `Form` to percent-encode using the `attr-char` rules.
    pub(crate) fn percent_encode_attr_chars(mut self) -> Self {
        self.percent_encoding = PercentEncoding::AttrChar;
        self
    }

    /// Configure this `Form` to skip percent-encoding
    pub(crate) fn percent_encode_noop(mut self) -> Self {
        self.percent_encoding = PercentEncoding::NoOp;
        self
    }

    // If predictable, computes the length the request will have
    // The length should be preditable if only String and file fields have been added,
    // but not if a generic reader has been added;
    pub(crate) fn compute_length(&mut self) -> Option<u64> {
        let mut length = 0u64;
        for &(ref name, ref field) in self.fields.iter() {
            match field.value_len() {
                Some(value_length) => {
                    // We are constructing the header just to get its length. To not have to
                    // construct it again when the request is sent we cache these headers.
                    let header = self.percent_encoding.encode_headers(name, field.metadata());
                    let header_length = header.len();
                    self.computed_headers.push(header);
                    // The additions mimick the format string out of which the field is constructed
                    // in Reader. Not the cleanest solution because if that format string is
                    // ever changed then this formula needs to be changed too which is not an
                    // obvious dependency in the code.
                    length += 2 + self.boundary().len() as u64 + 2 + header_length as u64 + 4 + value_length + 2
                }
                _ => return None,
            }
        }
        // If there is a at least one field there is a special boundary for the very last field.
        if !self.fields.is_empty() {
            length += 2 + self.boundary().len() as u64 + 4
        }
        Some(length)
    }

    /// Take the fields vector of this instance, replacing with an empty vector.
    fn take_fields(&mut self) -> Vec<(Cow<'static, str>, P)> {
        std::mem::replace(&mut self.fields, Vec::new())
    }
}

impl<P: fmt::Debug> FormParts<P> {
    pub(crate) fn fmt_fields(&self, ty_name: &'static str, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct(ty_name)
            .field("boundary", &self.boundary)
            .field("parts", &self.fields)
            .finish()
    }
}

// ===== impl PartMetadata =====

impl PartMetadata {
    pub(crate) fn new() -> Self {
        PartMetadata {
            mime: None,
            file_name: None,
            headers: HeaderMap::default()
        }
    }

    pub(crate) fn mime(mut self, mime: Mime) -> Self {
        self.mime = Some(mime);
        self
    }

    pub(crate) fn file_name<T>(mut self, filename: T) -> Self
    where
        T: Into<Cow<'static, str>>,
    {
        self.file_name = Some(filename.into());
        self
    }
}


impl PartMetadata {
    pub(crate) fn fmt_fields<'f, 'fa, 'fb>(
        &self,
        debug_struct: &'f mut fmt::DebugStruct<'fa, 'fb>
    ) -> &'f mut fmt::DebugStruct<'fa, 'fb> {
        debug_struct
            .field("mime", &self.mime)
            .field("file_name", &self.file_name)
            .field("headers", &self.headers)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct AttrCharEncodeSet;

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

pub(crate) enum PercentEncoding {
    PathSegment,
    AttrChar,
    NoOp,
}

impl PercentEncoding {
    pub(crate) fn encode_headers(&self, name: &str, field: &PartMetadata) -> Vec<u8> {
        let s = format!(
            "Content-Disposition: form-data; {}{}{}",
            self.format_parameter("name", name),
            match field.file_name {
                Some(ref file_name) => format!("; {}", self.format_filename(file_name)),
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

    // According to RFC7578 Section 4.2, `filename*=` syntax is invalid.
    // See https://github.com/seanmonstar/reqwest/issues/419.
    fn format_filename(&self, filename: &str) -> String {
        let legal_filename = filename.replace("\\", "\\\\")
                                     .replace("\"", "\\\"")
                                     .replace("\r", "\\\r")
                                     .replace("\n", "\\\n");
        format!("filename=\"{}\"", legal_filename)
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
            PercentEncoding::NoOp => { value.to_string() },
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
        form.inner.boundary = "boundary".to_string();
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
        part.meta.headers.insert("Hdr3", "/a/b/c".parse().unwrap());
        let mut form = Form::new().part("key2", part);
        form.inner.boundary = "boundary".to_string();
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
            PercentEncoding::PathSegment.encode_headers(name, &field.meta),
            &b"Content-Disposition: form-data; name*=utf-8''start%25'%22%0D%0A%C3%9Fend"[..]
        );

        assert_eq!(
            PercentEncoding::AttrChar.encode_headers(name, &field.meta),
            &b"Content-Disposition: form-data; name*=utf-8''start%25%27%22%0D%0A%C3%9Fend"[..]
        );
    }
}
