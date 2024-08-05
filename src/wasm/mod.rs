#[cfg(feature = "wasm-component")]
pub mod component;
#[cfg(feature = "wasm-component")]
pub use component::*;
#[cfg(not(feature = "wasm-component"))]
pub mod js;
#[cfg(not(feature = "wasm-component"))]
pub use js::*;
