use core::ptr;

use security_framework::secure_transport::SslProtocolSide;

use super::{ssl_policy, ssl_policy_from_ref};

/// `SecPolicyCreateSSL` returns NULL when `Security.framework` cannot build a
/// policy, and the probe has to read that as "no platform verifier". Handing
/// the NULL to `core-foundation` instead raised the "Attempted to create a
/// NULL object" panic `pnpr` reported on its first resolve (pnpm/pnpm#14461).
#[test]
fn a_null_policy_ref_is_unavailable_instead_of_panicking() {
    assert!(ssl_policy_from_ref(ptr::null_mut()).is_none());
}

/// The other half of the same branch: a policy the framework did hand out is
/// adopted, so the probe continues to evaluate trust instead of falling back
/// on every machine.
#[test]
fn a_created_policy_ref_is_adopted() {
    let policy = ssl_policy(SslProtocolSide::SERVER, "registry.npmjs.org");

    assert!(policy.is_some());
}
