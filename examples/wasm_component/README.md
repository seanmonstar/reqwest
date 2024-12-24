# HTTP Reqwest

This is a simple Rust Wasm example that sends an outgoing http request using the `reqwest` library to [https://hyper.rs](https://hyper.rs).

## Prerequisites

- `cargo` 1.82+
- `rustup target add wasm32-wasip2`
- [wasmtime 23.0.0+](https://github.com/bytecodealliance/wasmtime)

## Building

```bash
# Build Wasm component
cargo build --target wasm32-wasip2
```

## Running with wasmtime

```bash
wasmtime serve -Scommon ./target/wasm32-wasip2/debug/http_reqwest.wasm
```

Then send a request to `localhost:8080`

```bash
> curl localhost:8080

<!doctype html>
<html>
<head>
    <title>Example Domain</title>
....
```
