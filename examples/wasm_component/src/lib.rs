wit_bindgen::generate!();

use exports::wasi::http::incoming_handler::Guest;
use wasi::http::types::*;

struct ReqwestComponent;

impl Guest for ReqwestComponent {
    fn handle(_request: IncomingRequest, response_out: ResponseOutparam) {
        let response = OutgoingResponse::new(Fields::new());
        response.set_status_code(200).unwrap();
        let response_body = response.body().unwrap();
        ResponseOutparam::set(response_out, Ok(response));

        let exampledotcom = reqwest::Client::new().get("http://example.com").send();
        let response = futures::executor::block_on(exampledotcom).expect("should get response");
        let bytes = futures::executor::block_on(response.bytes()).expect("should get bytes");

        response_body
            .write()
            .unwrap()
            .blocking_write_and_flush(&bytes)
            .unwrap();
        OutgoingBody::finish(response_body, None).expect("failed to finish response body");
    }
}

export!(ReqwestComponent);
