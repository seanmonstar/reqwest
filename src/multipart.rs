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
            value: Box::new(BytesReader::new(value.into())),
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
                Some(ref mime) => format!("\r\n{}", ::header::ContentType(mime.clone())),
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
    field_position: usize,
    active_reader: Box<Read + Send>,
}

// TODO: MultipartField cannot derive debug because active_reader is not Debug
// Not sure how to best resolve this...
impl std::fmt::Debug for RequestReader {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "")
    }
}

#[derive(Debug)]
struct BytesReader<T> {
    position: usize,
    bytes: T,
}

impl<T> BytesReader<T> {
    fn new(bytes: T) -> BytesReader<T> {
        BytesReader {
            position: 0,
            bytes: bytes,
        }
    }
}

impl<T: AsRef<[u8]>> Read for BytesReader<T> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let bytes = self.bytes.as_ref();
        let bytes_remaining = bytes.len() - self.position;
        let bytes_to_write = std::cmp::min(bytes_remaining, buf.len());
        // unwrap because the range is always valid
        buf.get_mut(0..bytes_to_write)
            .unwrap()
            .copy_from_slice(&bytes[self.position..self.position + bytes_to_write]);
        self.position += bytes_to_write;
        Ok(bytes_to_write)
    }
}

impl RequestReader {
    fn new(request: MultipartRequest) -> RequestReader {
        let mut reader = RequestReader {
            request: request,
            field_position: 0,
            active_reader: Box::new(std::io::empty()),
        };
        reader.update_reader();
        reader
    }
    fn update_reader(&mut self) {
        self.active_reader = if self.field_position < self.request.fields.len() {
            // We need to move out of the vector here because we are consuming the field's reader
            let field = self.request.fields.remove(self.field_position);
            Box::new(
                BytesReader::new(format!(
                    "\r\n--{}\r\n{}\r\n\r\n",
                    self.request.boundary,
                    field.header()
                )).chain(field.value),
            )
        } else {
            Box::new(std::io::empty())
        }
    }
}

impl Read for RequestReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut total_bytes_read = 0usize;
        loop {
            if self.field_position < self.request.fields.len() {
                // unwrap because the range is always valid
                total_bytes_read += self.active_reader
                    .read(buf.get_mut(total_bytes_read..).unwrap())?;
                if total_bytes_read == buf.len() {
                    // buffer is full
                    // reader might or might not be done
                    // (in case reader had exactly buf.len() bytes left)
                    return Ok(total_bytes_read);
                } else {
                    // active_reader did not fill the buffer, so it must be done
                    // switch to the next reader
                    self.field_position += 1;
                    self.update_reader();
                }
            } else {
                return Ok(0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_reader_empty() {
        let mut reader = BytesReader::new(Vec::new());
        let mut output = Vec::new();
        assert_eq!(reader.read_to_end(&mut output).unwrap(), 0);
        assert_eq!(output.len(), 0);
    }

    #[test]
    fn bytes_reader_empty_0_read() {
        let mut reader = BytesReader::new(Vec::new());
        let mut output = [];
        // Read into 0 length buffer twice
        assert_eq!(reader.read(&mut output).unwrap(), 0);
        assert_eq!(reader.read(&mut output).unwrap(), 0);
    }

    #[test]
    fn bytes_reader_read_to_end() {
        let input = [0, 1];
        let mut reader = BytesReader::new(input.clone());
        let mut output = Vec::new();
        assert_eq!(reader.read_to_end(&mut output).unwrap(), 2);
        assert_eq!(output, input);
    }

    #[test]
    fn bytes_reader_multiple_reads() {
        let input = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
        let mut reader = BytesReader::new(input.clone());
        let mut output = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(reader.read(&mut output[0..0]).unwrap(), 0);
        assert_eq!(reader.read(&mut output[0..2]).unwrap(), 2);
        assert_eq!(reader.read(&mut output[0..0]).unwrap(), 0);
        assert_eq!(reader.read(&mut output[2..]).unwrap(), 8);
        assert_eq!(reader.read(&mut output[10..11]).unwrap(), 0);
        assert_eq!(output[..10], input);
    }
}
