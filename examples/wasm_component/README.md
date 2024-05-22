# HTTP Reqwest

This is a simple Rust Wasm example that sends an outgoing http request using the `reqwest` library to [https://example.com](https://example.com).

## Prerequisites

- `cargo` 1.75+
- [wasm-tools](https://github.com/bytecodealliance/wasm-tools)
- [wasmtime](https://github.com/bytecodealliance/wasmtime) >=20.0.0
- `wasi_snapshot_preview1.reactor.wasm` adapter, downloaded from [wasmtime release](https://github.com/bytecodealliance/wasmtime/releases/tag/v20.0.0)

## Building

```bash
# Build Wasm module
cargo build --release --target wasm32-wasi
# Create a Wasm component from the Wasm module by using the adapter
wasm-tools component new ./target/wasm32-wasi/release/http_reqwest.wasm -o ./component.wasm --adapt ./wasi_snapshot_preview1.reactor.wasm
```

## Running with wasmtime

```bash
wasmtime serve -Scommon ./component.wasm
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
