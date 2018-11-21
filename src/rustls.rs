use std::{ fmt, error };
use std::io::Cursor;
use untrusted::Input;
use tokio_rustls::webpki::trust_anchor_util::cert_der_as_trust_anchor;
use rustls::internal::pemfile;

#[derive(Debug)]
pub struct Error {}

impl fmt::Display for Error {
    fn fmt(&self, fer: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(self, fer)
    }
}

impl error::Error for Error {
    fn description(&self) -> &str {
        "TLS Error"
    }
}

/// Represent an X509 certificate.
pub struct Certificate(rustls::Certificate);

impl Certificate {
    /// Create a `Certificate` from a binary DER encoded certificate
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::fs::File;
    /// # use std::io::Read;
    /// # fn cert() -> Result<(), Box<std::error::Error>> {
    /// let mut buf = Vec::new();
    /// File::open("my_cert.der")?
    ///     .read_to_end(&mut buf)?;
    /// let cert = reqwest::Certificate::from_der(&buf)?;
    /// # drop(cert);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// If the provided buffer is not valid DER, an error will be returned.
    pub fn from_der(der: &[u8]) -> ::Result<Certificate> {
        Certificate::from_der_vec(der.into())
    }

    /// Create a `Certificate` from a PEM encoded certificate
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::fs::File;
    /// # use std::io::Read;
    /// # fn cert() -> Result<(), Box<std::error::Error>> {
    /// let mut buf = Vec::new();
    /// File::open("my_cert.pem")?
    ///     .read_to_end(&mut buf)?;
    /// let cert = reqwest::Certificate::from_pem(&buf)?;
    /// # drop(cert);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// If the provided buffer is not valid PEM, an error will be returned.
    pub fn from_pem(der: &[u8]) -> ::Result<Certificate> {
        let mut pem = Cursor::new(der);
        pemfile::certs(&mut pem)
            .map_err(|_| ::error::from(Error {}))?
            .into_iter()
            .find_map(|rustls::Certificate(der)| Certificate::from_der_vec(der).ok())
            .ok_or_else(|| ::error::from(Error {}))
    }

    fn from_der_vec(der: Vec<u8>) -> ::Result<Certificate> {
        let ret = {
            let input = Input::from(&der);
            cert_der_as_trust_anchor(input).is_ok()
        };

        if ret {
            Ok(Certificate(rustls::Certificate(der)))
        } else {
            Err(::error::from(Error {}))
        }
    }

    pub(crate) fn cert(self) -> rustls::Certificate {
        self.0
    }
}

impl fmt::Debug for Certificate {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Certificate")
            .finish()
    }
}


/// Represent a private key and X509 cert as a client certificate.
pub struct Identity(rustls::PrivateKey, rustls::Certificate);

impl Identity {
    /// TODO
    pub fn from_pem(pem: &[u8]) -> ::Result<Identity> {
        let mut pem = Cursor::new(pem);
        let mut certs = try_!(pemfile::certs(&mut pem)
            .map_err(|_| Error {}));
        pem.set_position(0);
        let mut sk = try_!(pemfile::pkcs8_private_keys(&mut pem)
            .or_else(|_| {
                pem.set_position(0);
                pemfile::rsa_private_keys(&mut pem)
            })
            .map_err(|_| Error {}));

        if let (Some(sk), Some(pk)) = (sk.pop(), certs.pop()) {
            Ok(Identity(sk, pk))
        } else {
            Err(::error::from(Error {}))
        }
    }

    pub(crate) fn into_inner(self) -> (rustls::PrivateKey, rustls::Certificate) {
        (self.0, self.1)
    }
}

impl fmt::Debug for Identity {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Identity")
            .finish()
    }
}
