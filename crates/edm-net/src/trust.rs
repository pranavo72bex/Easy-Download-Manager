// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

//! Certificates trusted beside the public roots: the benchmark's own server, whose certificate
//! is made at run time and named by `EDM_BENCH_CA` (bench/README.md, "The line cases"). edm-core
//! reads the switch and hands the file here ([`trust_also`]) before the first client is built;
//! [`tls_config`](crate::http_client::tls_config) adds them to its store when it is made, once,
//! and from then on none can be added.

use tokio_rustls::rustls::pki_types::pem::PemObject;
use tokio_rustls::rustls::pki_types::CertificateDer;
use tokio_rustls::rustls::RootCertStore;

/// The certificates trusted beside the public roots, and whether the TLS set-up that holds them
/// has been made (sealed): one added after it would never be in force.
#[derive(Debug, Default)]
pub(crate) struct AlsoTrusted {
    sealed: bool,
    roots: Vec<CertificateDer<'static>>,
}

impl AlsoTrusted {
    /// Adds the certificates of `pem` (one or more, PEM), each checked as a root a store takes;
    /// none of them where one is not, or where the set-up is already made.
    pub(crate) fn add(&mut self, pem: &[u8]) -> Result<(), crate::Error> {
        if self.sealed {
            return Err(crate::Error::Internal(
                "TLS: a certificate to trust came after EDM's TLS was set up: not in force".into(),
            ));
        }
        self.roots.extend(certificates(pem)?);
        Ok(())
    }

    /// The certificates, with no more taken from now on.
    pub(crate) fn seal(&mut self) -> Vec<CertificateDer<'static>> {
        self.sealed = true;
        self.roots.clone()
    }
}

/// The one list the program's TLS set-up reads.
static ALSO_TRUSTED: std::sync::Mutex<AlsoTrusted> = std::sync::Mutex::new(AlsoTrusted {
    sealed: false,
    roots: Vec::new(),
});

/// Trusts the certificates of `pem` (PEM text, one or more) beside the public roots, for every
/// client and stream protocol EDM builds. Only before EDM's TLS is first set up: called after,
/// it is an error that says so, and nothing is added. A file that holds no certificate, or one
/// a store refuses, is an error too.
pub fn trust_also(pem: &[u8]) -> Result<(), crate::Error> {
    crate::lock(&ALSO_TRUSTED).add(pem)
}

/// The certificates [`trust_also`] was given, for the TLS set-up being made now; none can be
/// added after.
pub(crate) fn sealed_roots() -> Vec<CertificateDer<'static>> {
    crate::lock(&ALSO_TRUSTED).seal()
}

/// The certificates of `pem`, each one a root store takes; an error for text that holds none or
/// one that is not a certificate.
pub(crate) fn certificates(pem: &[u8]) -> Result<Vec<CertificateDer<'static>>, crate::Error> {
    let mut found = Vec::new();
    for cert in CertificateDer::pem_slice_iter(pem) {
        let cert = cert.map_err(|e| crate::Error::Internal(format!("TLS: {e}")))?;
        RootCertStore::empty()
            .add(cert.clone())
            .map_err(|e| crate::Error::Internal(format!("TLS: not a root to trust: {e}")))?;
        found.push(cert);
    }
    if found.is_empty() {
        return Err(crate::Error::Internal(
            "TLS: the file holds no certificate (PEM)".into(),
        ));
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A certificate made for the test, as PEM.
    fn made() -> String {
        rcgen::generate_simple_self_signed(vec!["localhost".into()])
            .unwrap()
            .cert
            .pem()
    }

    #[test]
    fn certificates_are_added_until_the_set_up_is_made_and_refused_after() {
        let mut also = AlsoTrusted::default();
        also.add(made().as_bytes()).unwrap();
        let two = format!("{}{}", made(), made());
        also.add(two.as_bytes()).unwrap();
        assert_eq!(also.seal().len(), 3);
        let said = format!("{:?}", also.add(made().as_bytes()).unwrap_err());
        assert!(said.contains("after EDM's TLS was set up"), "{said}");
        assert_eq!(also.seal().len(), 3);
    }

    #[test]
    fn text_with_no_certificate_or_a_broken_one_is_refused_and_adds_nothing() {
        let mut also = AlsoTrusted::default();
        let none = format!("{:?}", also.add(b"not a certificate").unwrap_err());
        assert!(none.contains("holds no certificate"), "{none}");
        // Base64 that reads, but three zero bytes are no certificate.
        let broken = b"-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n";
        assert!(also.add(broken).is_err());
        // A good one beside a broken one: neither is taken.
        let mixed = format!("{}{}", made(), String::from_utf8_lossy(broken));
        assert!(also.add(mixed.as_bytes()).is_err());
        assert!(also.seal().is_empty());
    }
}
