use std::fmt;
use native_tls;

/// Represent an X509 certificate.
pub struct Certificate(native_tls::Certificate);

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
        let inner = try_!(native_tls::Certificate::from_der(der));
        Ok(Certificate(inner))
    }
}

impl fmt::Debug for Certificate {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Certificate")
            .finish()
    }
}

// pub(crate)

pub fn cert(cert: Certificate) -> native_tls::Certificate {
    cert.0
}


/// Represent a PKCS12 bundle containing a private key and X509 cert.
pub struct Pkcs12(native_tls::Pkcs12);

impl Pkcs12 {
    /// Parses a DER-formatted PKCS #12 archive, using the specified password to decrypt the key.
    ///
    /// The archive should contain a leaf certificate and its private key, as well any intermediate
    /// certificates that allow clients to build a chain to a trusted root.
    /// The chain certificates should be in order from the leaf certificate towards the root.
    ///
    /// PKCS #12 archives typically have the file extension `.p12` or `.pfx`, and can be created
    /// with the OpenSSL `pkcs12` tool:
    ///
    /// ```bash
    /// openssl pkcs12 -export -out identity.pfx -inkey key.pem -in cert.pem -certfile chain_certs.pem
    /// ```
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::fs::File;
    /// # use std::io::Read;
    /// # fn pkcs12() -> Result<(), Box<std::error::Error>> {
    /// let mut buf = Vec::new();
    /// File::open("my-ident.pfx")?
    ///     .read_to_end(&mut buf)?;
    /// let pkcs12 = reqwest::Pkcs12::from_der(&buf, "my-privkey-password")?;
    /// # drop(pkcs12);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// If the provided buffer is not valid DER, an error will be returned.
    pub fn from_der(der: &[u8], password: &str) -> ::Result<Pkcs12> {
        let inner = try_!(native_tls::Pkcs12::from_der(der, password));
        Ok(Pkcs12(inner))
    }
}

impl fmt::Debug for Pkcs12 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Pkcs12")
            .finish()
    }
}

// pub(crate)

pub fn pkcs12(pkcs12: Pkcs12) -> native_tls::Pkcs12 {
    pkcs12.0
}