#[cfg(feature = "default-tls")]
mod default;
#[cfg(feature = "rustls-tls")]
mod rustls;

#[cfg(feature = "default-tls")]
use ::native_tls::{ TlsConnector, TlsConnectorBuilder };

pub use self::default::{Certificate, Identity};
// pub use self::rustls::{Certificate, Identity};

pub enum TlsBackend {
    #[cfg(feature = "default-tls")]
    Default(TlsConnectorBuilder),
    #[cfg(feature = "rustls-tls")]
    Rustls(::rustls::ClientConfig)
}

impl Default for TlsBackend {
    fn default() -> TlsBackend {
        #[cfg(feature = "default-tls")]
        { TlsBackend::Default(TlsConnector::builder()) }

        #[cfg(all(feature = "rustls-tls", not(feature = "default-tls")))]
        { TlsBackend::Rustls(::rustls::ClientConfig::new()) }
    }
}
