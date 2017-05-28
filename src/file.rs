use std::path::Path;

use hyper::mime;

#[derive(Clone)]
pub struct File<'a> {
    pub name: String,
    pub path: &'a Path,
    pub mime: Option<mime::Mime>,
}
