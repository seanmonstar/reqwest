#[cfg(all(target_os = "wasi", target_env = "p2"))]
pub mod component;
#[cfg(all(target_os = "wasi", target_env = "p2"))]
pub use component::*;

#[cfg(not(all(target_os = "wasi", target_env = "p2")))]
pub mod js;
#[cfg(not(all(target_os = "wasi", target_env = "p2")))]
pub use js::*;
