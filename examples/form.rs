// Short example of a POST request with form data.
//
// This is using the `tokio` runtime. You'll need the following dependency:
//
// `tokio = { version = "1", features = ["full"] }`
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
#[tokio::main]
async fn main() {
    let response = reqwest::Client::new()
        .post("http://www.baidu.com")
        .form(&[("one", "1")])
        .send()
        .await
        .expect("send");
    println!("Response status {}", response.status());
}

// The [cfg(not(all(target_arch = "wasm32", target_os = "unknown")))] above prevent building the tokio::main function
// for wasm32 target, because tokio isn't compatible with wasm32.
// If you aren't building for wasm32, you don't need that line.
// The two lines below avoid the "'main' function not found" error when building for wasm32 target.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn main() {}
