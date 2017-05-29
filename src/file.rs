use std::path::Path;

use hyper::mime;

/// A `File` to send with the `multipart` method.
#[derive(Clone)]
pub struct File<'a> {
    /// The name of the file in the multipart/form-data request.
    pub name: String,
    /// The path of the file on your filesystem.
    pub path: &'a Path,
    /// The mime of the file for in the multipart/form-data request.
    pub mime: Option<mime::Mime>,
}
