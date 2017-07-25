extern crate url;
extern crate uuid;

use std;
use std::borrow::Cow;
use std::io::Read;
use hyper::mime::Mime;

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

    fn reader(self) -> MultipartReader {
        MultipartReader::new(self)
    }

    /// Gets the automatically chosen boundary.
    pub fn boundary(&self) -> &str {
        &self.boundary
    }

    fn compute_length(&self) -> Option<u64> {
        let mut length = 0u64;
        for field in self.fields.iter() {
            match field.value_length {
                Some(value_length) => {
                    // We are constructing the header just to get its length. This is wasteful
                    // but it seemed liked the only way to get the correct length for sure.
                    // TODO: Should we instead cache the computed header for when it is used again
                    // when sending the request? This would use more memory as all headers would be
                    // held in memory as opposed to only the current one but it would only compute
                    // each header once.
                    let header_length = field.header().len();
                    // The additions mimick the format string out of which the field is constructed
                    // in MultipartReader. Not the cleanest solution because if that format string is
                    // ever changed then this formula needs to be changed too which is not an
                    // obvious dependency in the code.
                    length += 2 + self.boundary.len() as u64 + 2 + header_length as u64 + 4 + value_length + 2
                }
                _ => return None,
            }
        }
        // If there is a at least one field there is a special boundary for the very last field.
        if self.fields.len() != 0 {
            length += 2 + self.boundary.len() as u64 + 2
        }
        Some(length)
    }
}

/// Turns this MultipartRequest into a MultipartReader which implements the Read trait.
pub fn reader(request: MultipartRequest) -> MultipartReader {
    request.reader()
}

/// If predictable, computes the length the request will have
/// The length should be preditable if only String and file fields have been added,
/// but not if a generic reader has been added;
pub fn compute_length(request: &MultipartRequest) -> Option<u64> {
    request.compute_length()
}

/// A field in a multipart request.
pub struct MultipartField {
    name: Cow<'static, str>,
    value: Box<Read + Send>,
    value_length: Option<u64>,
    mime: Option<Mime>,
    file_name: Option<Cow<'static, str>>,
}

impl std::fmt::Debug for MultipartField {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("MultipartField")
            .field("name", &self.name)
            .field("value_length", &self.value_length)
            .field("mime", &self.mime)
            .field("file_name", &self.file_name)
            .finish()
    }
}

impl MultipartField {
    /// Makes a String parameter.
    ///
    /// ```
    /// reqwest::MultipartField::param("key", "value");
    /// ```
    ///
    /// ```
    /// let key: String = "key".to_string();
    /// let value: String = "value".to_string();
    /// reqwest::MultipartField::param(key, value);
    /// ```
    ///
    pub fn param<T: Into<Cow<'static, str>>, U: AsRef<[u8]> + Send + 'static>(name: T, value: U) -> MultipartField {
        let value_length = Some(value.as_ref().len() as u64);
        MultipartField {
            name: name.into(),
            value: Box::new(std::io::Cursor::new(value)),
            value_length: value_length,
            mime: None,
            file_name: None,
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
    /// ```no_run
    /// use std::fs::File;
    /// let file = File::open("foo.txt").unwrap();
    /// reqwest::MultipartField::reader("key", file);
    /// ```
    ///
    pub fn reader<T: Into<Cow<'static, str>>, U: Read + Send + 'static>(name: T, value: U) -> MultipartField {
        MultipartField {
            name: name.into(),
            value: Box::from(value),
            value_length: None,
            mime: None,
            file_name: None,
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
    pub fn file<T: Into<Cow<'static, str>>, U: AsRef<std::path::Path>>(
        name: T,
        path: U,
    ) -> std::io::Result<MultipartField> {
        let file_name = path.as_ref().file_name().and_then(|filename| {
            Some(Cow::from(filename.to_string_lossy().into_owned()))
        });
        let file = std::fs::File::open(path)?;
        let file_length = file.metadata().ok().map(|meta| meta.len());
        Ok(MultipartField {
            name: name.into(),
            value: Box::new(file),
            value_length: file_length,
            mime: Some(::hyper::mime::APPLICATION_OCTET_STREAM),
            file_name: file_name,
        })
    }

    /// Sets the mime, builder style.
    ///
    /// ```
    /// use reqwest::mime;
    /// reqwest::MultipartField::param("key", "value").mime(mime::IMAGE_BMP);
    /// ```
    ///
    /// ```
    /// use reqwest::mime;
    /// reqwest::MultipartField::param("key", "value").mime(None);
    /// ```
    ///
    pub fn mime<T: Into<Option<Mime>>>(mut self, mime: T) -> MultipartField {
        self.mime = mime.into();
        self
    }

    /// Sets the filename, builder style.
    ///
    /// ```
    /// reqwest::MultipartField::param("key", "value").file_name(Some("filename"));
    /// ```
    ///
    /// ```
    /// let file_name = "file_name".to_string();
    /// reqwest::MultipartField::param("key", "value").file_name(Some(file_name));
    /// ```
    ///
    pub fn file_name<T: Into<Cow<'static, str>>>(mut self, filename: Option<T>) -> MultipartField {
        self.file_name = filename.and_then(|filename| Some(filename.into()));
        self
    }

    fn header(&self) -> String {
        fn format_parameter(name: &str, value: &str) -> String {
            // let legal_value = percent_encode(value);
            let legal_value = url::percent_encoding::utf8_percent_encode(value, url::percent_encoding::PATH_SEGMENT_ENCODE_SET).to_string();
            if value.len() == legal_value.len() {
                // nothing has been percent encoded
                format!("{}=\"{}\"", name, value)
            } else {
                // something has been percent encoded
                format!("{}*=utf-8''{}", name, legal_value)
            }
        }

        format!(
            "Content-Disposition: form-data; {}{}{}",
            format_parameter("name", self.name.as_ref()),
            match self.file_name {
                Some(ref file_name) => format!("; {}", format_parameter("filename", file_name)),
                None => String::new(),
            },
            match self.mime {
                Some(ref mime) => {
                    format!("\r\nContent-Type: {}", mime)
                }
                None => "".to_string(),
            }
        )
    }
}

pub struct MultipartReader {
    request: MultipartRequest,
    active_reader: Option<Box<Read + Send>>,
}

impl std::fmt::Debug for MultipartReader {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("MultipartReader")
            .field("request", &self.request)
            .finish()
    }
}

impl MultipartReader {
    fn new(request: MultipartRequest) -> MultipartReader {
        let mut reader = MultipartReader {
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
                Some(Box::new(reader.chain(std::io::Cursor::new(
                    format!("--{}--", self.request.boundary),
                ))))
            }
        } else {
            None
        }
    }
}

impl Read for MultipartReader {
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
    use std::collections::HashMap;
    use super::*;
    use multipart::ser::to_multipart;

    #[test]
    fn multipart_request_empty() {
        let mut output = Vec::new();
        let request = MultipartRequest::new();
        let length = request.compute_length();
        request.reader().read_to_end(&mut output).unwrap();
        assert_eq!(output, b"");
        assert_eq!(length.unwrap(), 0);
    }

    #[test]
    fn multipart_request_read_to_end() {
        let mut output = Vec::new();
        let mut request = MultipartRequest::new()
            .field(MultipartField::reader("reader1", std::io::empty()))
            .field(MultipartField::param("key1", "value1"))
            .field(
                MultipartField::param("key2", "value2").mime(Some(::mime::IMAGE_BMP)),
            )
            .field(MultipartField::reader("reader2", std::io::empty()))
            .field(
                MultipartField::param("key3", "value3").file_name(Some("filename")),
            );
        request.boundary = "boundary".to_string();
        let length = request.compute_length();
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
                        value3\r\n--boundary--";
        request.reader().read_to_end(&mut output).unwrap();
        // These prints are for debug purposes in case the test fails
        println!(
            "START REAL\n{}\nEND REAL",
            std::str::from_utf8(&output).unwrap()
        );
        println!("START EXPECTED\n{}\nEND EXPECTED", expected);
        assert_eq!(std::str::from_utf8(&output).unwrap(), expected);
        assert!(length.is_none());
    }

    #[test]
    fn multipart_request_read_to_end_with_length() {
        let mut output = Vec::new();
        let mut request = MultipartRequest::new()
            .field(MultipartField::param("key1", "value1"))
            .field(
                MultipartField::param("key2", "value2").mime(Some(::mime::IMAGE_BMP)),
            )
            .field(
                MultipartField::param("key3", "value3").file_name(Some("filename")),
            );
        request.boundary = "boundary".to_string();
        let length = request.compute_length();
        let expected = "--boundary\r\n\
                        Content-Disposition: form-data; name=\"key1\"\r\n\r\n\
                        value1\r\n\
                        --boundary\r\n\
                        Content-Disposition: form-data; name=\"key2\"\r\n\
                        Content-Type: image/bmp\r\n\r\n\
                        value2\r\n\
                        --boundary\r\n\
                        Content-Disposition: form-data; name=\"key3\"; filename=\"filename\"\r\n\r\n\
                        value3\r\n--boundary--";
        request.reader().read_to_end(&mut output).unwrap();
        // These prints are for debug purposes in case the test fails
        println!(
            "START REAL\n{}\nEND REAL",
            std::str::from_utf8(&output).unwrap()
        );
        println!("START EXPECTED\n{}\nEND EXPECTED", expected);
        assert_eq!(std::str::from_utf8(&output).unwrap(), expected);
        assert_eq!(length.unwrap(), expected.len() as u64);
    }

    const EXPECTED_SERIALIZE: &str = "--boundary\r\n\
                                      Content-Disposition: form-data; name=\"name\"\r\n\r\n\
                                      Sean\r\n\
                                      --boundary\r\n\
                                      Content-Disposition: form-data; name=\"age\"\r\n\r\n\
                                      5\r\n\
                                      --boundary--";

    #[test]
    fn multipart_header_percent_encoding() {
        let field = MultipartField::param("start%'\"\r\nßend", "");
        let expected = "Content-Disposition: form-data; name*=utf-8''start%25\'%22%0D%0A%C3%9Fend";

        assert_eq!(field.header(), expected);
    }

    #[test]
    fn multipart_serializer_struct() {
        #[derive(Serialize)]
        struct A {
            name: &'static str,
            age: u8,
        };

        let mut request = to_multipart(A {
            name: "Sean",
            age: 5,
        }).unwrap();
        request.boundary = "boundary".to_string();

        let mut output = String::new();
        request.reader().read_to_string(&mut output).unwrap();

        assert_eq!(output.as_str(), EXPECTED_SERIALIZE);
    }

    #[test]
    fn multipart_serializer_list() {
        let mut request = to_multipart(&[("name", "Sean"), ("age", "5")]).unwrap();
        request.boundary = "boundary".to_string();

        let mut output = String::new();
        request.reader().read_to_string(&mut output).unwrap();

        assert_eq!(output.as_str(), EXPECTED_SERIALIZE);
    }

    #[test]
    fn multipart_serializer_map() {
        let mut map = HashMap::new();
        map.insert("name", "Sean");
        map.insert("age", "5");

        let mut request = to_multipart(&map).unwrap();
        request.boundary = "boundary".to_string();

        let mut output = String::new();
        request.reader().read_to_string(&mut output).unwrap();

        // Can't do equals comparison here because the HashMap is unordered and might yield the
        // parameters in a different order than expected
        assert!(output.as_str().contains("name=\"name\"\r\n\r\nSean\r\n"));
        assert!(output.as_str().contains("name=\"age\"\r\n\r\n5\r\n"));
    }
}
