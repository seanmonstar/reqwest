extern crate uuid;
use std;
use std::io::Read;
use hyper::mime::Mime;

// TODO: Should all these Strings be String, &str or Cow?

// TODO: error management
#[derive(Debug)]
pub enum Error {
}

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

/// A multi/formdata request
/// TODO: better documentation
#[derive(Debug)]
pub struct MultipartRequest {
    /// The boundary used in the request
    pub boundary: String,
    fields: Vec<Field>,
}

impl MultipartRequest {
    /// Create a new MultipartRequest without any content
    pub fn new() -> MultipartRequest {
        MultipartRequest {
            boundary: format!("{}", uuid::Uuid::new_v4().simple()),
            fields: Vec::new(),
        }
    }
    // TODO: ergonomic methods like add_param, add_file, ...
    /// Turn this MultipartRequest into a RequestReader which implemetns the Read trait
    pub fn reader(self) -> RequestReader {
        RequestReader::new(self)
    }
}

struct Field {
    name: String,
    value: Box<Read + Send>,
    mime: Option<Mime>,
    filename: Option<Filename>,
}

// TODO: Field cannot derive debug because value is not Debug
// Not sure how to best resolve this...
impl std::fmt::Debug for Field {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "")
    }
}

impl Field {
    // TODO: This should use the hyper::Header infrastructure
    fn header(&self) -> String {
        // TODO: The RFC says name can be any utf8 but
        // wouldnt it be a problem if name or filename contained a " (quoation mark)here?
        format!(
            "Content-Disposition: form-data; name=\"{}\"{}{}",
            self.name,
            match self.filename {
                Some(ref filename) => format!("; filename=\"{}\"", filename.encode()),
                None => "".to_string(),
            },
            match self.mime {
                Some(ref mime) => format!("\r\nContent-Type: {}", mime),
                None => "".to_string(),
            }
        )
    }
}

// TODO: Is any utf8 even allowed here?
// The RFC makes it sound like only ascii excluding control sequences is allowed
#[allow(dead_code)] // TODO: Remove this. Added so project compiles since warnings=errors
#[derive(Debug)]
pub enum Filename {
    Utf8(String),
    Binary(Vec<u8>),
}

impl Filename {
    fn encode(&self) -> String {
        match self {
            &Filename::Utf8(ref name) => name.clone(),
            // TODO: implement percent encoding
            &Filename::Binary(_) => unimplemented!(),
        }
    }
}

pub struct RequestReader {
    request: MultipartRequest,
    field_position: usize,
    active_reader: Box<Read + Send>,
}

// TODO: Field cannot derive debug because value is not Debug
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

#[derive(Debug)]
struct EmptyReader;

impl Read for EmptyReader {
    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
        Ok(0)
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
            active_reader: Box::new(EmptyReader),
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
            Box::new(EmptyReader)
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
    fn empty_reader_is_empty() {
        let mut reader = EmptyReader;
        let mut output = Vec::new();
        assert_eq!(reader.read_to_end(&mut output).unwrap(), 0);
        assert_eq!(output.len(), 0);
    }

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
        let input = [0,1];
        let mut reader = BytesReader::new(input.clone());
        let mut output = Vec::new();
        assert_eq!(reader.read_to_end(&mut output).unwrap(), 2);
        assert_eq!(output, input);
    }

    #[test]
    fn bytes_reader_multiple_reads() {
        let input = [0,1,2,3,4,5,6,7,8,9];
        let mut reader = BytesReader::new(input.clone());
        let mut output = [0,0,0,0,0,0,0,0,0,0,0];
        assert_eq!(reader.read(&mut output[0..0]).unwrap(), 0);
        assert_eq!(reader.read(&mut output[0..2]).unwrap(), 2);
        assert_eq!(reader.read(&mut output[0..0]).unwrap(), 0);
        assert_eq!(reader.read(&mut output[2..]).unwrap(), 8);
        assert_eq!(reader.read(&mut output[10..11]).unwrap(), 0);
        assert_eq!(output[..10], input);
    }
}
