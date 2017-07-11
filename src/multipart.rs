extern crate uuid;

use std;
use std::io::Read;
use hyper::mime::Mime;

// TODO: error management
// I dont know if I need to tie in with error.rs since currently the errors are limited to this
// module and will not appear anywhere else (RequestBuilder::multipart does not error).
#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
}

impl std::convert::From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Error {
        Error::Io(err)
    }
}

type Result<T> = std::result::Result<T, Error>;

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "")
    }
}

impl std::error::Error for Error {
    fn description(&self) -> &str {
        ""
    }

    fn cause(&self) -> Option<&std::error::Error> {
        None
    }
}

/// A multipart/form-data request.
#[derive(Debug)]
pub struct MultipartRequest {
    boundary: String,
    fields: Vec<MultipartField>,
}

impl MultipartRequest {
    /// Creates a new MultipartRequest without any content.
    pub fn new() -> MultipartRequest {
        MultipartRequest {
            boundary: format!("{}", uuid::Uuid::new_v4().simple()),
            fields: Vec::new(),
        }
    }
    /// Adds a field, builder style.
    pub fn field(mut self, field: MultipartField) -> MultipartRequest {
        self.fields.push(field);
        self
    }
    /// Adds multiple fields.
    pub fn fields(&mut self, mut fields: Vec<MultipartField>) {
        self.fields.append(&mut fields);
    }
    /// Turns this MultipartRequest into a RequestReader which implements the Read trait.
    pub fn reader(self) -> RequestReader {
        RequestReader::new(self)
    }
    /// Gets the automatically chosen boundary.
    pub fn boundary(&self) -> &str {
        return &self.boundary;
    }
}

/// A field in a multipart request.
pub struct MultipartField {
    name: String,
    value: Box<Read + Send>,
    mime: Option<Mime>,
    filename: Option<Filename>,
}

// TODO: MultipartField cannot derive debug because value is not Debug
// Not sure how to best resolve this...
impl std::fmt::Debug for MultipartField {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "")
    }
}

impl MultipartField {
    /// Makes a String parameter.
    ///
    /// ```
    /// reqwest::MultipartField::param("key", "value");
    /// ```
    ///
    pub fn param<T: Into<String>, U: Into<String>>(name: T, value: U) -> MultipartField {
        MultipartField {
            name: name.into(),
            value: Box::new(std::io::Cursor::new(value.into())),
            mime: None,
            filename: None,
        }
    }
    /// Adds a generic reader.
    ///
    /// ```
    /// use std::io::empty;
    /// let reader = empty();
    /// reqwest::MultipartField::reader("key", reader);
    /// ```
    ///
    pub fn reader<T: Into<String>, U: Read + Send + 'static>(name: T, value: U) -> MultipartField {
        MultipartField {
            name: name.into(),
            value: Box::from(value),
            mime: None,
            filename: None,
        }
    }
    /// Makes a file parameter.
    /// Defaults to mime type application/octet-stream.
    ///
    /// ```no_run
    /// reqwest::MultipartField::file("key", "/path/to/file");
    /// ```
    ///
    /// # Errors
    /// Errors when the file cannot be opened.
    pub fn file<T: Into<String>, U: AsRef<std::path::Path>>(name: T, path: U) -> Result<MultipartField> {
        // This turns the path into a filename if possible.
        // TODO: If the path's OsStr cannot be converted to a String it will result in None
        // instead of Filename::Bytes because I found no waz to convert an OsStr into bytes.
        let filename = path.as_ref()
            .file_name()
            .and_then(|filename| filename.to_str())
            .and_then(|filename| Some(Filename::Utf8(filename.to_string())));
        Ok(MultipartField {
            name: name.into(),
            value: Box::new(std::fs::File::open(path)?),
            mime: Some(::hyper::mime::APPLICATION_OCTET_STREAM),
            filename: filename,
        })
    }
    /// Sets the mime, builder style.
    ///
    /// ```
    /// use reqwest::mime;
    /// reqwest::MultipartField::param("key", "value").mime(Some(mime::IMAGE_BMP));
    /// ```
    ///
    pub fn mime(mut self, mime: Option<Mime>) -> MultipartField {
        self.mime = mime;
        self
    }
    /// Sets the filename, builder style.
    ///
    /// ```
    /// reqwest::MultipartField::param("key", "value").filename(Some("filename"));
    /// ```
    ///
    pub fn filename<T: Into<String>>(mut self, filename: Option<T>) -> MultipartField {
        self.filename = filename.and_then(|filename| Some(Filename::Utf8(filename.into())));
        self
    }
    fn header(&self) -> String {
        // TODO: The RFC says name can be any utf8 but wouldnt it be a problem if name or filename
        // contained a " (quoation mark)here?
        // TODO: I would use hyper's ContentDisposition header here, but it doesnt seem to have
        // the form-data type
        format!(
            "Content-Disposition: form-data; name=\"{}\"{}{}",
            self.name,
            match self.filename {
                Some(ref filename) => format!("; filename=\"{}\"", filename.encode()),
                None => "".to_string(),
            },
            match self.mime {
                Some(ref mime) => {
                    format!(
                        // TODO: Apparently I still have to write out Content-Type here?!
                        // I thought header would format itself that way on its own
                        "\r\nContent-Type: {}",
                        ::header::ContentType(mime.clone())
                    )
                }
                None => "".to_string(),
            }
        )
    }
}

#[derive(Debug)]
pub enum Filename {
    // TODO: Is any utf8 even allowed here?
    // The RFC makes it sound like only ascii excluding control sequences is allowed
    Utf8(String),
    // TODO: Currently unused because we never construct it
    #[allow(dead_code)]
    Bytes(Vec<u8>),
}

impl Filename {
    fn encode(&self) -> String {
        match self {
            &Filename::Utf8(ref name) => name.clone(),
            // TODO: implement percent encoding
            &Filename::Bytes(_) => unimplemented!(),
        }
    }
}

pub struct RequestReader {
    request: MultipartRequest,
    active_reader: Option<Box<Read + Send>>,
}

// TODO: MultipartField cannot derive debug because active_reader is not Debug
// Not sure how to best resolve this...
impl std::fmt::Debug for RequestReader {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "")
    }
}

impl RequestReader {
    fn new(request: MultipartRequest) -> RequestReader {
        let mut reader = RequestReader {
            request: request,
            active_reader: None,
        };
        reader.next_reader();
        reader
    }
    fn next_reader(&mut self) {
        self.active_reader = if self.request.fields.len() != 0 {
            // We need to move out of the vector here because we are consuming the field's reader
            let field = self.request.fields.remove(0);
            let reader = std::io::Cursor::new(format!(
                "--{}\r\n{}\r\n\r\n",
                self.request.boundary,
                field.header()
            )).chain(field.value)
                .chain(std::io::Cursor::new("\r\n"));
            // According to https://tools.ietf.org/html/rfc2046#section-5.1.1
            // the very last field has a special boundary
            if self.request.fields.len() != 0 {
                Some(Box::new(reader))
            } else {
                Some(Box::new(
                    reader.chain(std::io::Cursor::new(format!("--{}--", self.request.boundary))),
                ))
            }
        } else {
            None
        }
    }
}

impl Read for RequestReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut total_bytes_read = 0usize;
        let mut last_read_bytes;
        loop {
            match self.active_reader {
                Some(ref mut reader) => {
                    // unwrap because the range is always valid
                    last_read_bytes = reader.read(buf.get_mut(total_bytes_read..).unwrap())?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multipart_request_empty() {
        let mut output = Vec::new();
        MultipartRequest::new()
            .reader()
            .read_to_end(&mut output)
            .unwrap();
        assert_eq!(output, b"");
    }

    #[test]
    fn multipart_request_read_to_end() {
        let mut output = Vec::new();
        let mut a = MultipartRequest::new()
            .field(MultipartField::reader("reader1", std::io::empty()))
            .field(MultipartField::param("key1", "value1"))
            .field(
                MultipartField::param("key2", "value2").mime(Some(::mime::IMAGE_BMP)),
            )
            .field(MultipartField::reader("reader2", std::io::empty()))
            .field(
                MultipartField::param("key3", "value3").filename(Some("filename")),
            );
        a.boundary = "boundary".to_string();
        let expected = "\
                        --boundary\r\n\
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
                        value3\r\n--boundary--";
        a.reader().read_to_end(&mut output).unwrap();
        // These prints are for debug purposes in case the test fails
        println!(
            "START REAL\n{}\nEND REAL",
            std::str::from_utf8(&output).unwrap()
        );
        println!("START EXPECTED\n{}\nEND EXPECTED", expected);
        assert_eq!(std::str::from_utf8(&output).unwrap(), expected);
    }
}
