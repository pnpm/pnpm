use super::{Certificate, Identity, LazyLock, RegistryTls, TlsConfig, TlsError};

/// Which trust anchors a client built by
/// [`ThrottledClient::for_installs`](crate::ThrottledClient::for_installs) verifies registry certificates
/// against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TrustRoots {
    /// The OS trust store, via reqwest's `rustls-platform-verifier`
    /// backend. Picks up corporate / MITM roots an administrator
    /// installed system-wide, which the bundled set cannot.
    Platform,

    /// The Mozilla root set compiled into the binary. Used on Android,
    /// where the platform verifier requires a JVM, and as a fallback when
    /// the platform trust store cannot be loaded. On other platforms a
    /// loadable system store retains its administrator's trust decisions.
    Bundled,
}

/// The Mozilla CA root set compiled into the binary, in reqwest's
/// [`Certificate`] form. Parsed once — `for_installs` builds one
/// client per per-registry override, and the fallback path is taken
/// by every one of them on a system without a trust store.
pub(super) fn bundled_root_certs() -> &'static [Certificate] {
    static CERTS: LazyLock<Vec<Certificate>> = LazyLock::new(|| {
        webpki_root_certs::TLS_SERVER_ROOT_CERTS
            .iter()
            .map(|der| Certificate::from_der(der).expect("bundled Mozilla root is valid DER"))
            .collect()
    });
    &CERTS
}

/// Load the PEM bundle named by `NODE_EXTRA_CA_CERTS` as extra trust
/// roots, to be added to every client `for_installs` builds.
///
/// `NODE_EXTRA_CA_CERTS` is the standard Node convention for appending
/// a CA to the default trust store. pnpm-on-Node inherits that trust
/// implicitly because it runs inside Node; pacquet is a native binary,
/// so to keep real-world parity for users behind a corporate MITM proxy
/// it reads the variable explicitly. This is the one deliberate
/// exception to the ".npmrc-only, no env vars" TLS parity policy
/// documented in [`tls::TlsConfig`](crate::tls::TlsConfig): the variable is a process-global
/// Node convention rather than a pnpm setting, and Node already honors
/// it for pnpm today — so reading it *restores* parity rather than
/// diverging from it. The certs are added in
/// [`ThrottledClient::for_installs`](crate::ThrottledClient::for_installs) (not [`apply_tls`]) so the
/// `.npmrc`-derived [`TlsConfig`] stays env-free.
///
/// Read and parsed once per [`ThrottledClient::for_installs`](crate::ThrottledClient::for_installs) call —
/// that constructor builds one client per per-registry override, so
/// loading here (rather than inside the per-client builder) avoids
/// re-reading and re-parsing the bundle N times during startup.
///
/// The resulting certs are additive and lowest-priority: layered under
/// the `.npmrc` `ca` / `cafile` roots that [`apply_tls`] adds afterward
/// and under the platform trust store (ordering is immaterial — the
/// rustls root store is a union). A missing, unreadable, or malformed
/// file yields an empty list, matching pnpm's silent treatment of a
/// missing `cafile` rather than failing the client build.
pub(super) fn load_node_extra_ca_certs() -> Vec<Certificate> {
    let Some(path) = std::env::var_os("NODE_EXTRA_CA_CERTS").filter(|value| !value.is_empty())
    else {
        return Vec::new();
    };
    let Ok(bytes) = std::fs::read(&path) else {
        return Vec::new();
    };
    parse_ca_bundle(&bytes)
}

/// The certificates carried by one PEM buffer.
///
/// Material the TLS backend cannot read is skipped rather than
/// rejected. Node ignores such material instead of refusing to open a
/// TLS context, so pnpm 11 installs fine with an empty, truncated, or
/// `${VAR}`-unresolved `ca=` value; pacquet drops the same material
/// instead of failing the client build (pnpm/pnpm#14646).
///
/// `Certificate::from_pem_bundle` reads the buffer as a unit and fails
/// it whole, so one corrupt block in a bundle would take the roots
/// around it down with it. Falling back to a block-by-block read keeps
/// every certificate that is readable on its own.
pub(super) fn parse_ca_bundle(pem: &[u8]) -> Vec<Certificate> {
    if let Ok(certs) = Certificate::from_pem_bundle(pem) {
        return certs;
    }
    String::from_utf8_lossy(pem)
        .split_inclusive(END_CERTIFICATE)
        .filter_map(|block| Certificate::from_pem_bundle(block.as_bytes()).ok())
        .flatten()
        .collect()
}

/// The PEM armor that closes a certificate. Splitting a bundle on it
/// is how pnpm's own `cafile` reader delimits one certificate from the
/// next.
const END_CERTIFICATE: &str = "-----END CERTIFICATE-----";

/// Build the effective [`TlsConfig`] for a per-registry override:
/// each scoped field (`ca`, `cert`, `key`) replaces its top-level
/// counterpart field-by-field; `strict_ssl` and `local_address`
/// always come from the top-level (only `:cert(file)?` / `:key(file)?`
/// / `:ca(file)?` are recognized as per-registry keys).
///
/// The `ca` field is special: a per-registry `ca` is stored as a
/// single string that may contain multiple concatenated PEMs, while
/// the top-level `ca` is a `Vec<String>` (the `cafile` loader split).
/// When the override has a `ca`, the effective top-level CA list is
/// *replaced* (not merged) by a one-element list with the scoped PEM
/// blob — which [`parse_ca_bundle`] handles fine since it accepts
/// multi-cert PEM buffers.
pub(super) fn merge_tls(top: &TlsConfig, override_: &RegistryTls) -> TlsConfig {
    TlsConfig {
        ca: match &override_.ca {
            Some(pem) => vec![pem.clone()],
            None => top.ca.clone(),
        },
        cert: override_.cert.clone().or_else(|| top.cert.clone()),
        key: override_.key.clone().or_else(|| top.key.clone()),
        strict_ssl: top.strict_ssl,
        local_address: top.local_address,
    }
}

/// Apply [`TlsConfig`] onto a [`reqwest::ClientBuilder`]: register each
/// CA, install the client identity, set `danger_accept_invalid_certs`
/// when `strict_ssl: false`, and pin the outbound interface. Returns
/// the modified builder unchanged when every field is `None` / empty —
/// matching pnpm's "TLS-unset is default-TLS" semantics.
///
/// `strict_ssl` defaults to `true` here (`unwrap_or(true)`) rather than
/// in the config layer because that's where pnpm applies the same
/// default — see the "Defaults" section of [`TlsConfig`]. A `cert` /
/// `key` pair rustls rejects surfaces as
/// [`TlsError::InvalidClientIdentity`] and bubbles through
/// [`ForInstallsError`](crate::ForInstallsError), the way Node throws from
/// `tls.createSecureContext`. Unreadable `ca` material is dropped
/// instead — see [`parse_ca_bundle`].
pub(super) fn apply_tls(
    mut builder: reqwest::ClientBuilder,
    tls: &TlsConfig,
) -> Result<reqwest::ClientBuilder, TlsError> {
    for pem in &tls.ca {
        for cert in parse_ca_bundle(pem.as_bytes()) {
            builder = builder.add_root_certificate(cert);
        }
    }
    let cert = drop_blank(tls.cert.as_deref());
    let key = drop_blank(tls.key.as_deref());
    if let (Some(cert), Some(key)) = (cert, key) {
        // reqwest's `Identity::from_pem` (gated on the `rustls`
        // feature pacquet builds with) takes a single PEM buffer
        // containing *both* the certificate and the private key, in
        // any order. Concatenating with a `\n` separator handles
        // both pnpm-style configs (where `cert=` and `key=` arrive
        // separately) and users who paste them into one field.
        //
        // rustls accepts PKCS#1 (`-----BEGIN RSA PRIVATE KEY-----`),
        // PKCS#8 (`-----BEGIN PRIVATE KEY-----`), and EC
        // (`-----BEGIN EC PRIVATE KEY-----`) private keys — same
        // surface area Node's `tls.createSecureContext` exposes,
        // and the surface pnpm hands to undici. PKCS#12 (`.pfx`) is
        // not supported by pnpm at the config layer (no `pfx=`
        // option in pnpm's `.npmrc` allow-list), so pacquet doesn't
        // need to handle it either.
        let combined = format!("{cert}\n{key}");
        let identity = Identity::from_pem(combined.as_bytes())
            .map_err(|source| TlsError::InvalidClientIdentity { reason: source.to_string() })?;
        builder = builder.identity(identity);
    }
    // The `strict-ssl` default is `true`, applied here at client-build
    // time rather than at config-parse time.
    if !tls.strict_ssl.unwrap_or(true) {
        builder = builder.danger_accept_invalid_certs(true);
    }
    if let Some(addr) = tls.local_address {
        builder = builder.local_address(addr);
    }
    Ok(builder)
}

/// `None` for a PEM slot that is empty or all whitespace.
///
/// A blank setting is how an unset one looks: a `cert=` line a config
/// generator wrote out with nothing after it, or a `key=${CORP_KEY}`
/// whose variable never resolved. pnpm tests these fields for
/// truthiness before handing them to undici, so a blank one never
/// reaches Node's TLS layer; pacquet drops it at the same point rather
/// than pair it with its counterpart and fail the install on the
/// identity rustls then rejects (pnpm/pnpm#14646).
fn drop_blank(pem: Option<&str>) -> Option<&str> {
    pem.filter(|pem| !pem.trim().is_empty())
}
