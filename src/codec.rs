//! This module contains traits for encoding and decoding HTTP bodies
//!
//! It also contains implementations based on reqwest features:
//!   * `json` - Json encoding and decoding

use serde::{de::DeserializeOwned, ser::Serialize};

/// A trait describing the ability to encode an HTTP body
pub trait Encoder {
    /// The error potentially returned during encoding
    type Error: 'static + Send + Sync + std::error::Error;

    /// The `Content-Type` header value used during encoding
    const CONTENT_TYPE: &'static str;

    /// Encodes a `serde::ser::Serialize` type
    fn encode<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<Vec<u8>, Self::Error>;
}

/// A trait describing the ability to decode an HTTP body
pub trait Decoder {
    /// The error potentially returned during decoding
    type Error: 'static + Send + Sync + std::error::Error;

    /// Decods a `serde::de::DeserializeOwned` type
    fn decode<T: DeserializeOwned>(&mut self, bytes: &[u8]) -> Result<T, Self::Error>;
}

impl<E: Encoder> Encoder for &mut E {
    type Error = E::Error;

    const CONTENT_TYPE: &'static str = E::CONTENT_TYPE;

    #[inline]
    fn encode<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<Vec<u8>, Self::Error> {
        (*self).encode(value)
    }
}

impl<D: Decoder> Decoder for &mut D {
    type Error = D::Error;

    #[inline]
    fn decode<T: DeserializeOwned>(&mut self, bytes: &[u8]) -> Result<T, Self::Error> {
        (*self).decode(bytes)
    }
}

/// An encoder and decoder for JSON-formatted HTTP bodies
#[cfg(feature = "json")]
#[derive(Debug)]
pub struct Json;

#[cfg(feature = "json")]
impl Encoder for Json {
    type Error = serde_json::Error;

    const CONTENT_TYPE: &'static str = "application/json";

    #[inline]
    fn encode<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<Vec<u8>, Self::Error> {
        serde_json::to_vec(value)
    }
}

#[cfg(feature = "json")]
impl Decoder for Json {
    type Error = serde_json::Error;

    #[inline]
    fn decode<T: DeserializeOwned>(&mut self, bytes: &[u8]) -> Result<T, Self::Error> {
        serde_json::from_slice(bytes)
    }
}
