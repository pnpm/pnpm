use security_framework::{certificate::SecCertificate, trust::SecTrust};

use super::is_platform_verifier_available;

/// A policy `Security.framework` did hand out is adopted and stays usable,
/// so the probe goes on to evaluate trust instead of falling back on every
/// machine.
///
/// Whether the framework hands one out is a property of the host: it
/// returns NULL on the machines
/// [pnpm/pnpm#14461](https://github.com/pnpm/pnpm/issues/14461) reports. So
/// the test asserts the branch the host takes rather than one fixed
/// outcome. Where a policy was created it has to survive into `SecTrust`;
/// where the framework declined it, the probe has to report the platform
/// verifier as unavailable, which is the input that sends the client
/// builder to [`TrustRoots::Bundled`](super::TrustRoots::Bundled).
#[test]
fn a_created_policy_is_adopted() {
    let Some(policy) = pnpm_ffi_security::server_ssl_policy("registry.npmjs.org") else {
        assert!(!is_platform_verifier_available());

        return;
    };

    let der = &webpki_root_certs::TLS_SERVER_ROOT_CERTS[0];
    let cert = SecCertificate::from_der(der.as_ref()).expect("bundled Mozilla root is valid DER");

    assert!(SecTrust::create_with_certificates(&[cert], &[policy]).is_ok());
}
