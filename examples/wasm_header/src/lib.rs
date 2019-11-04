use wasm_bindgen::prelude::*;

// NOTE: This test is a clone of https://github.com/rustwasm/wasm-bindgen/blob/master/examples/fetch/src/lib.rs
// but uses Reqwest instead of the web_sys fetch api directly

/**
* curl --location --request POST "https://postman-echo.com/post" \
 --data "This is expected to be sent back as part of response body."
*/
#[wasm_bindgen]
pub async fn run() -> Result<JsValue, JsValue> {
    let res = reqwest::Client::new()
        .post("https://postman-echo.com/post")
        .body("This is expected to be sent back as part of response body.")
        .header("Content-Type", "application/x-www-form-urlencoded")
        // .header("Access-Control-Allow-Origin", "*")
        .send()
        .await?;

    let text = res.text().await?;

    Ok(JsValue::from_str(&text))
}
