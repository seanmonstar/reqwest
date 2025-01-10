use wasi::http::types::{
    Fields, IncomingBody, IncomingRequest, OutgoingBody, OutgoingResponse, ResponseOutparam,
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

        let mut response = reqwest::get("https://hyper.rs").expect("should get response bytes");
        let (mut body_stream, incoming_body) = response
            .bytes_stream()
            .expect("should be able to get response body stream");
        std::io::copy(
            &mut body_stream,
            &mut response_body
                .write()
                .expect("should be able to write to response body"),
        )
        .expect("should be able to stream input to output");
        drop(body_stream);
        IncomingBody::finish(incoming_body);
        OutgoingBody::finish(response_body, None).expect("failed to finish response body");
    }
}

wasi::http::proxy::export!(ReqwestComponent);
