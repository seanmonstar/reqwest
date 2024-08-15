use wasi::{
    http::types::{Fields, IncomingRequest, OutgoingBody, OutgoingResponse, ResponseOutparam},
    io::streams::{InputStream, OutputStream, StreamError},
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

        let response =
            futures::executor::block_on(reqwest::Client::new().get("https://hyper.rs").send())
                .expect("should get response bytes");
        let incoming_body = response.bytes_stream().expect("should get incoming body");
        let stream = incoming_body.stream().expect("should get bytes stream");
        stream_input_to_output(
            stream,
            response_body
                .write()
                .expect("should be able to write to response body"),
        )
        .expect("should be able to stream input to output");

        OutgoingBody::finish(response_body, None).expect("failed to finish response body");
    }
}

pub fn stream_input_to_output(data: InputStream, out: OutputStream) -> Result<(), StreamError> {
    loop {
        match out.blocking_splice(&data, u64::MAX) {
            Ok(bytes_spliced) if bytes_spliced == 0 => return Ok(()),
            Ok(_) => {}
            Err(e) => match e {
                StreamError::Closed => {
                    return Ok(());
                }
                StreamError::LastOperationFailed(e) => {
                    return Err(StreamError::LastOperationFailed(e));
                }
            },
        }
    }
}

wasi::http::proxy::export!(ReqwestComponent);
