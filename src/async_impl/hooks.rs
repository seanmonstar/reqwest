//! Hooks to intercept the request, response and response body

use bytes::Bytes;

use super::{Request, Response};

/// Hook that gets called before sending the request, right after it's constructed
pub trait RequestHook: Send + Sync {
    /// Intercept the request and return it with or without changes
    fn intercept(&self, req: Request) -> Request;
}

/// Hook that gets called once the request is completed and headers have been received
pub trait ResponseHook: Send + Sync {
    /// Intercept the response and return it with or without changes
    fn intercept(&self, res: Response) -> Response;
}

/// Hook that gets called once the request is completed and the full body has been received
pub trait ResponseBodyHook: Send + Sync {
    /// Intercept the response body and return it with or without changes
    fn intercept(&self, body: Bytes) -> Bytes;
}
