use wasi::http::types::{
    Fields, IncomingRequest, OutgoingBody, OutgoingResponse, ResponseOutparam,
};

#[allow(unused)]
struct ReqwestComponent;

impl wasi::exports::http::incoming_handler::Guest for ReqwestComponent {
    fn handle(_request: IncomingRequest, response_out: ResponseOutparam) {
        let response = OutgoingResponse::new(Fields::new());
        response.set_status_code(200).unwrap();
        let response_body = response
            .body()
            .expect("should be able to get response body");
        ResponseOutparam::set(response_out, Ok(response));

        let mut response =
            futures::executor::block_on(reqwest::Client::new().get("https://hyper.rs").send())
                .expect("should get response bytes");
        std::io::copy(
            &mut response.bytes_stream().expect("should get incoming body"),
            &mut response_body
                .write()
                .expect("should be able to write to response body"),
        )
        .expect("should be able to stream input to output");

        OutgoingBody::finish(response_body, None).expect("failed to finish response body");
    }
}

wasi::http::proxy::export!(ReqwestComponent);
