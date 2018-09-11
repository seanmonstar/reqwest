//! multipart/form-data
use std::borrow::Cow;
use std::fmt;
use std::fs::File;
use std::io::{self, Cursor, Read};
use std::path::Path;

use mime_guess::{self, Mime};
use url::percent_encoding;
use uuid::Uuid;
use http::HeaderMap;

use {Body};

/// A multipart/form-data request.
pub struct Form {
    boundary: String,
    fields: Vec<(Cow<'static, str>, Part)>,
    headers: Vec<Vec<u8>>,
}

impl Form {
    /// Creates a new Form without any content.
    pub fn new() -> Form {
        Form {
            boundary: format!("{}", Uuid::new_v4().to_simple()),
            fields: Vec::new(),
            headers: Vec::new(),
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
    /// let form = reqwest::multipart::Form::new()
    ///     .text("username", "seanmonstar")
    ///     .text("password", "secret");
    /// ```
    pub fn text<T, U>(self, name: T, value: U) -> Form
    where T: Into<Cow<'static, str>>,
          U: Into<Cow<'static, str>>,
    {
        self.part(name, Part::text(value))
    }

    /// Adds a file field.
    ///
    /// The path will be used to try to guess the filename and mime.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn run() -> ::std::io::Result<()> {
    /// let files = reqwest::multipart::Form::new()
    ///     .file("key", "/path/to/file")?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Errors when the file cannot be opened.
    pub fn file<T, U>(self, name: T, path: U) -> io::Result<Form>
    where T: Into<Cow<'static, str>>,
          U: AsRef<Path>
    {
        Ok(self.part(name, Part::file(path)?))
    }

    /// Adds a customized Part.
    pub fn part<T>(mut self, name: T, part: Part) -> Form
    where T: Into<Cow<'static, str>>,
    {
        self.fields.push((name.into(), part));
        self
    }

    pub(crate) fn reader(self) -> Reader {
        Reader::new(self)
    }

    // If predictable, computes the length the request will have
    // The length should be preditable if only String and file fields have been added,
    // but not if a generic reader has been added;
    pub(crate) fn compute_length(&mut self) -> Option<u64> {
        let mut length = 0u64;
        for &(ref name, ref field) in self.fields.iter() {
            match ::body::len(&field.value) {
                Some(value_length) => {
                    // We are constructing the header just to get its length. To not have to
                    // construct it again when the request is sent we cache these headers.
                    let header = header(name, field);
                    let header_length = header.len();
                    self.headers.push(header);
                    // The additions mimick the format string out of which the field is constructed
                    // in Reader. Not the cleanest solution because if that format string is
                    // ever changed then this formula needs to be changed too which is not an
                    // obvious dependency in the code.
                    length += 2 + self.boundary.len() as u64 + 2 + header_length as u64 + 4 + value_length + 2
                }
                _ => return None,
            }
        }
        // If there is a at least one field there is a special boundary for the very last field.
        if self.fields.len() != 0 {
            length += 2 + self.boundary.len() as u64 + 4
        }
        Some(length)
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

    /// Adds a generic reader.
    ///
    /// Does not set filename or mime.
    pub fn reader<T: Read + Send + 'static>(value: T) -> Part {
        Part::new(Body::new(value))
    }

    /// Adds a generic reader with known length.
    ///
    /// Does not set filename or mime.
    pub fn reader_with_length<T: Read + Send + 'static>(value: T, length: u64) -> Part {
        Part::new(Body::sized(value, length))
    }

    /// Makes a file parameter.
    ///
    /// # Errors
    ///
    /// Errors when the file cannot be opened.
    pub fn file<T: AsRef<Path>>(path: T) -> io::Result<Part> {
        let path = path.as_ref();
        let file_name = path.file_name().and_then(|filename| {
            Some(Cow::from(filename.to_string_lossy().into_owned()))
        });
        let ext = path.extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("");
        let mime = mime_guess::get_mime_type(ext);
        let file = File::open(path)?;
        let mut field = Part::new(Body::from(file));
        field.mime = Some(mime);
        field.file_name = file_name;
        Ok(field)
    }

    fn new(value: Body) -> Part {
        Part {
            value: value,
            mime: None,
            file_name: None,
            headers: HeaderMap::default()
        }
    }

    /// Sets the mime, builder style.
    pub fn mime(mut self, mime: Mime) -> Part {
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

pub(crate) struct Reader {
    form: Form,
    active_reader: Option<Box<Read + Send>>,
}

impl fmt::Debug for Reader {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Reader")
            .field("form", &self.form)
            .finish()
    }
}

impl Reader {
    fn new(form: Form) -> Reader {
        let mut reader = Reader {
            form: form,
            active_reader: None,
        };
        reader.next_reader();
        reader
    }

    fn next_reader(&mut self) {
        self.active_reader = if self.form.fields.len() != 0 {
            // We need to move out of the vector here because we are consuming the field's reader
            let (name, field) = self.form.fields.remove(0);
            let boundary = Cursor::new(format!("--{}\r\n", self.form.boundary));
            let header = Cursor::new({
                // Try to use cached headers created by compute_length
                let mut h = if self.form.headers.len() > 0 {
                    self.form.headers.remove(0)
                } else {
                    header(&name, &field)
                };
                h.extend_from_slice(b"\r\n\r\n");
                h
            });
            let reader = boundary
                .chain(header)
                .chain(::body::reader(field.value))
                .chain(Cursor::new("\r\n"));
            // According to https://tools.ietf.org/html/rfc2046#section-5.1.1
            // the very last field has a special boundary
            if self.form.fields.len() != 0 {
                Some(Box::new(reader))
            } else {
                Some(Box::new(reader.chain(Cursor::new(
                    format!("--{}--\r\n", self.form.boundary),
                ))))
            }
        } else {
            None
        }
    }
}

impl Read for Reader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut total_bytes_read = 0usize;
        let mut last_read_bytes;
        loop {
            match self.active_reader {
                Some(ref mut reader) => {
                    last_read_bytes = reader.read(&mut buf[total_bytes_read..])?;
                    total_bytes_read += last_read_bytes;
                    if total_bytes_read == buf.len() {
                        return Ok(total_bytes_read);
                    }
                }
                None => return Ok(total_bytes_read),
            };
            if last_read_bytes == 0 && buf.len() != 0 {
                self.next_reader();
            }
        }
    }
}


fn header(name: &str, field: &Part) -> Vec<u8> {
    let s = format!(
        "Content-Disposition: form-data; {}{}{}",
        format_parameter("name", name),
        match field.file_name {
            Some(ref file_name) => format!("; {}", format_parameter("filename", file_name)),
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

fn format_parameter(name: &str, value: &str) -> String {
    let legal_value =
        percent_encoding::utf8_percent_encode(value, percent_encoding::PATH_SEGMENT_ENCODE_SET)
            .to_string();
    if value.len() == legal_value.len() {
        // nothing has been percent encoded
        format!("{}=\"{}\"", name, value)
    } else {
        // something has been percent encoded
        format!("{}*=utf-8''{}", name, legal_value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_empty() {
        let mut output = Vec::new();
        let mut form = Form::new();
        let length = form.compute_length();
        form.reader().read_to_end(&mut output).unwrap();
        assert_eq!(output, b"");
        assert_eq!(length.unwrap(), 0);
    }

    #[test]
    fn read_to_end() {
        let mut output = Vec::new();
        let mut form = Form::new()
            .part("reader1", Part::reader(::std::io::empty()))
            .part("key1", Part::text("value1"))
            .part(
                "key2",
                Part::text("value2").mime(::mime::IMAGE_BMP),
            )
            .part("reader2", Part::reader(::std::io::empty()))
            .part(
                "key3",
                Part::text("value3").file_name("filename"),
            );
        form.boundary = "boundary".to_string();
        let length = form.compute_length();
        let expected = "--boundary\r\n\
                        Content-Disposition: form-data; name=\"reader1\"\r\n\r\n\
                        \r\n\
                        --boundary\r\n\
                        Content-Disposition: form-data; name=\"key1\"\r\n\r\n\
                        value1\r\n\
                        --boundary\r\n\
                        Content-Disposition: form-data; name=\"key2\"\r\n\
                        Content-Type: image/bmp\r\n\r\n\
                        value2\r\n\
                        --boundary\r\n\
                        Content-Disposition: form-data; name=\"reader2\"\r\n\r\n\
                        \r\n\
                        --boundary\r\n\
                        Content-Disposition: form-data; name=\"key3\"; filename=\"filename\"\r\n\r\n\
                        value3\r\n--boundary--\r\n";
        form.reader().read_to_end(&mut output).unwrap();
        // These prints are for debug purposes in case the test fails
        println!(
            "START REAL\n{}\nEND REAL",
            ::std::str::from_utf8(&output).unwrap()
        );
        println!("START EXPECTED\n{}\nEND EXPECTED", expected);
        assert_eq!(::std::str::from_utf8(&output).unwrap(), expected);
        assert!(length.is_none());
    }

    #[test]
    fn read_to_end_with_length() {
        let mut output = Vec::new();
        let mut form = Form::new()
            .text("key1", "value1")
            .part(
                "key2",
                Part::text("value2").mime(::mime::IMAGE_BMP),
            )
            .part(
                "key3",
                Part::text("value3").file_name("filename"),
            );
        form.boundary = "boundary".to_string();
        let length = form.compute_length();
        let expected = "--boundary\r\n\
                        Content-Disposition: form-data; name=\"key1\"\r\n\r\n\
                        value1\r\n\
                        --boundary\r\n\
                        Content-Disposition: form-data; name=\"key2\"\r\n\
                        Content-Type: image/bmp\r\n\r\n\
                        value2\r\n\
                        --boundary\r\n\
                        Content-Disposition: form-data; name=\"key3\"; filename=\"filename\"\r\n\r\n\
                        value3\r\n--boundary--\r\n";
        form.reader().read_to_end(&mut output).unwrap();
        // These prints are for debug purposes in case the test fails
        println!(
            "START REAL\n{}\nEND REAL",
            ::std::str::from_utf8(&output).unwrap()
        );
        println!("START EXPECTED\n{}\nEND EXPECTED", expected);
        assert_eq!(::std::str::from_utf8(&output).unwrap(), expected);
        assert_eq!(length.unwrap(), expected.len() as u64);
    }

    #[test]
    fn read_to_end_with_header() {
        let mut output = Vec::new();
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
        form.reader().read_to_end(&mut output).unwrap();
        // These prints are for debug purposes in case the test fails
        println!(
            "START REAL\n{}\nEND REAL",
            ::std::str::from_utf8(&output).unwrap()
        );
        println!("START EXPECTED\n{}\nEND EXPECTED", expected);
        assert_eq!(::std::str::from_utf8(&output).unwrap(), expected);
    }

    #[test]
    fn header_percent_encoding() {
        let name = "start%'\"\r\nßend";
        let field = Part::text("");
        let expected = "Content-Disposition: form-data; name*=utf-8''start%25\'%22%0D%0A%C3%9Fend";

        assert_eq!(header(name, &field), expected.as_bytes());
    }
}
