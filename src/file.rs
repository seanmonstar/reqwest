use std::path::PathBuf;

use hyper::mime;

/// A `File` to send with the `multipart` method.
#[derive(Clone)]
pub struct File {
    /// The name of the file in the multipart/form-data request.
    pub name: String,
    /// The path of the file on your filesystem.
    pub path: PathBuf,
    /// The mime of the file for in the multipart/form-data request.
    pub mime: mime::Mime,
}

impl File {
    /// Constructs a new File.
    pub fn new(name: String, path: PathBuf, mime: mime::Mime) -> File {
        File {
            name: name,
            path: path,
            mime: mime,
        }
    }
}
