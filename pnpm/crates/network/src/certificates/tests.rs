use core::ptr;

use security_framework::{
    certificate::SecCertificate, secure_transport::SslProtocolSide, trust::SecTrust,
};

use super::{is_platform_verifier_available, ssl_policy, ssl_policy_from_ref};

/// `SecPolicyCreateSSL` returns NULL when `Security.framework` cannot build a
/// policy, and the probe has to read that as "no platform verifier". Handing
/// the NULL to `core-foundation` instead raised the "Attempted to create a
/// NULL object" panic `pnpr` reported on its first resolve (pnpm/pnpm#14461).
#[test]
fn a_null_policy_ref_is_unavailable_instead_of_panicking() {
    assert!(ssl_policy_from_ref(ptr::null_mut()).is_none());
}

/// The other half of the same branch: a policy `Security.framework` did hand
/// out is adopted and stays usable, so the probe goes on to evaluate trust
/// instead of falling back on every machine.
///
/// Whether the framework hands one out is a property of the host — it returns
/// NULL on the machines `pnpm/pnpm#14461` reports — so the test asserts the
/// branch the host takes rather than one fixed outcome. Where a policy was
/// created it has to survive into `SecTrust`; where the framework declined it,
/// the probe has to report the platform verifier as unavailable, which is the
/// input that sends the client builder to
/// [`TrustRoots::Bundled`](super::TrustRoots::Bundled).
#[test]
fn a_created_policy_ref_is_adopted() {
    let Some(policy) = ssl_policy(SslProtocolSide::SERVER, "registry.npmjs.org") else {
        assert!(!is_platform_verifier_available());

        return;
    };

    let der = &webpki_root_certs::TLS_SERVER_ROOT_CERTS[0];
    let cert = SecCertificate::from_der(der.as_ref()).expect("bundled Mozilla root is valid DER");

    assert!(SecTrust::create_with_certificates(&[cert], &[policy]).is_ok());
}
