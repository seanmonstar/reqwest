use std::fmt;
use std::io::Cursor;
use untrusted::Input;
use tokio_rustls::webpki;
use tokio_rustls::webpki::trust_anchor_util::cert_der_as_trust_anchor;
use rustls::internal::pemfile;
use rustls::TLSError;


/// Represent an X509 certificate.
pub struct Certificate(::rustls::Certificate);

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
            .map_err(|_| ::error::from(TLSError::General(String::from("No valid certificate was found"))))?
            .into_iter()
            .find_map(|::rustls::Certificate(der)| Certificate::from_der_vec(der).ok())
            .ok_or_else(|| ::error::from(TLSError::General(String::from("No valid certificate was found"))))
    }

    fn from_der_vec(der: Vec<u8>) -> ::Result<Certificate> {
        let ret = {
            let input = Input::from(&der);
            cert_der_as_trust_anchor(input).is_ok()
        };

        if ret {
            Ok(Certificate(::rustls::Certificate(der)))
        } else {
            Err(::error::from(TLSError::WebPKIError(webpki::Error::BadDER)))
        }
    }

    pub(crate) fn cert(self) -> ::rustls::Certificate {
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
pub struct Identity(::rustls::PrivateKey, Vec<::rustls::Certificate>);

impl Identity {
    /// Parses PEM encoded private key and certificate.
    ///
    /// The input should contain a PEM encoded private key
    /// and at least one PEM encoded certificate.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::fs::File;
    /// # use std::io::Read;
    /// # fn pem() -> Result<(), Box<std::error::Error>> {
    /// let mut buf = Vec::new();
    /// File::open("my-ident.pem")?
    ///     .read_to_end(&mut buf)?;
    /// let id = reqwest::Identity::from_pem(&buf)?;
    /// # drop(id);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// If the provided buffer is not valid PEM, an error will be returned.
    pub fn from_pem(pem: &[u8]) -> ::Result<Identity> {
        let mut pem = Cursor::new(pem);
        let certs = try_!(pemfile::certs(&mut pem)
            .map_err(|_| TLSError::General(String::from("No valid certificate was found"))));
        pem.set_position(0);
        let mut sk = try_!(pemfile::pkcs8_private_keys(&mut pem)
            .or_else(|_| {
                pem.set_position(0);
                pemfile::rsa_private_keys(&mut pem)
            })
            .map_err(|_| TLSError::General(String::from("No valid private key was found"))));

        if let (Some(sk), false) = (sk.pop(), certs.is_empty()) {
            Ok(Identity(sk, certs))
        } else {
            Err(::error::from(TLSError::General(String::from("private key or certificate not found"))))
        }
    }

    pub(crate) fn into_inner(self) -> (::rustls::PrivateKey, Vec<::rustls::Certificate>) {
        (self.0, self.1)
    }
}

impl fmt::Debug for Identity {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Identity")
            .finish()
    }
}
