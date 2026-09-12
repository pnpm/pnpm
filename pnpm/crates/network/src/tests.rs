//! Tests for [`super`]'s proxy plumbing.
//!
//! Covers the proxy behaviors that don't require a real proxy listener:
//!
//! * `HTTP proxy` — per-URL routing, basic-auth decoding, scheme bypass.
//! * `SOCKS proxy` — routing decision (live-network case skipped).
//! * `noProxy` — reverse-dot-segment match, bypass-all literal.
//! * `Invalid proxy URL` — `ERR_PNPM_INVALID_PROXY`.
//!
//! The one HTTP integration test stands up a [`mockito`] server playing
//! the role of an HTTP proxy and asserts the request arrives with an
//! absolute-form URI and a decoded `Proxy-Authorization` header.

use super::{
    AuthHeaders, CappedDnsResolver, ForInstallsError, NetworkSettings, NoProxyMatcher,
    NoProxySetting, PerRegistryTls, ProxyConfig, ProxyError, ThrottledClient, TlsConfig,
    bundled_root_certs, nerf_dart, origin_of, parse_proxy_url, percent_decode_str,
};
use crate::proxy::strip_userinfo;
use pnpm_testing_utils::env_guard::EnvGuard;
use reqwest::{
    Url,
    dns::{Addrs, Name, Resolve, Resolving},
};
use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::Semaphore;

struct RecordingResolver {
    active: Arc<AtomicUsize>,
    gate: Arc<Semaphore>,
    maximum_active: Arc<AtomicUsize>,
}

impl Resolve for RecordingResolver {
    fn resolve(&self, _name: Name) -> Resolving {
        let active = Arc::clone(&self.active);
        let gate = Arc::clone(&self.gate);
        let maximum_active = Arc::clone(&self.maximum_active);
        Box::pin(async move {
            let active_count = active.fetch_add(1, Ordering::SeqCst) + 1;
            maximum_active.fetch_max(active_count, Ordering::SeqCst);
            let _gate_permit =
                gate.acquire_owned().await.expect("test gate semaphore is never closed");
            active.fetch_sub(1, Ordering::SeqCst);
            Ok(Box::new(std::iter::empty()) as Addrs)
        })
    }
}

fn list(entries: &[&str]) -> NoProxySetting {
    NoProxySetting::List(entries.iter().map(|s| (*s).to_string()).collect())
}

// --- TLS / local-address tests ---

/// Minimal self-signed certificate used to assert `Certificate::from_pem`
/// accepts well-formed PEM. Lives at
/// `crates/network/tests/fixtures/test-ca.pem` rather than inline so
/// the base64-encoded body stays out of the typos linter's word
/// dictionary. Regenerate with:
///
/// ```text
/// openssl req -x509 -newkey rsa:2048 -nodes -days 36500 \
///     -subj '/CN=pacquet-test' -keyout /dev/null \
///     -out crates/network/tests/fixtures/test-ca.pem
/// ```
///
/// The private key is discarded — only the cert is committed so the
/// workspace doesn't carry real key material. Each regeneration
/// produces a different cert (fresh keypair, fresh serial); that's
/// fine because nothing pins a specific issuer / fingerprint, the
/// fixture only needs to be a valid X.509 PEM that
/// `Certificate::from_pem` accepts.
const TEST_CA_PEM: &str = include_str!("../tests/fixtures/test-ca.pem");

// --- PKCS#1 / rustls regression tests ---

/// PKCS#1 client cert + key fixture. Generated with:
///
/// ```text
/// openssl genrsa -traditional -out crates/network/tests/fixtures/test-client-pkcs1.key 2048
/// openssl req -new -x509 -key crates/network/tests/fixtures/test-client-pkcs1.key \
///     -days 36500 -subj '/CN=pacquet-pkcs1-test' \
///     -out crates/network/tests/fixtures/test-client-pkcs1.crt
/// ```
///
/// The `-traditional` flag pins openssl to PKCS#1 (`-----BEGIN RSA
/// PRIVATE KEY-----`) instead of the default PKCS#8 — which is the
/// whole point of the regression test below. The cert and key are
/// self-signed and committed so the test stays deterministic.
const TEST_CLIENT_PKCS1_CERT: &str = include_str!("../tests/fixtures/test-client-pkcs1.crt");
const TEST_CLIENT_PKCS1_KEY: &str = include_str!("../tests/fixtures/test-client-pkcs1.key");

fn client_with_fetch_timeout(fetch_timeout: Duration) -> ThrottledClient {
    let settings = NetworkSettings { fetch_timeout, ..NetworkSettings::default() };
    ThrottledClient::for_installs(
        &ProxyConfig::default(),
        &TlsConfig::default(),
        &PerRegistryTls::default(),
        &settings,
    )
    .expect("custom fetch timeout builds")
}

/// Read a stalled body until the deadline surfaces as a timeout error.
async fn drain_until_timed_out(
    stream: &mut (impl futures_util::Stream<Item = std::io::Result<bytes::Bytes>> + Unpin),
) {
    use futures_util::StreamExt as _;

    loop {
        let chunk = stream.next().await.expect("deadline must surface as a body error");
        if let Err(error) = chunk {
            assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
            return;
        }
    }
}

mod behavior_capped_dns_resolver_limits;

mod behavior_a_body_that_keeps;

mod security;

mod configuration;

mod authorization;

mod manifests;

mod files;

mod streaming;
