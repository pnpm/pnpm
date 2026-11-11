//! Creating an SSL trust policy, and reading a refusal as a refusal.

use core_foundation::{base::TCFType as _, string::CFString};
use security_framework::policy::SecPolicy;
use security_framework_sys::{base::SecPolicyRef, policy::SecPolicyCreateSSL};

/// A server-side SSL policy for `hostname`, or `None` when
/// `Security.framework` declines to create one.
///
/// `SecPolicy::create_ssl` cannot report that refusal: it hands the C
/// function's return value to `core-foundation`'s create-rule wrapper,
/// which asserts the reference is non-NULL and panics otherwise. A
/// framework that hands out no policy is one of the shapes a
/// restricted `trustd` takes, so the C function is called directly and the
/// NULL is read as "no policy".
///
/// Catching the panic instead would not do: releases are built with
/// `panic = "abort"`, which turns the unwind into the very abort this
/// avoids.
#[must_use]
pub fn server_ssl_policy(hostname: &str) -> Option<SecPolicy> {
    let hostname = CFString::new(hostname);
    // SAFETY: `SecPolicyCreateSSL` takes the server flag by value and reads
    // the hostname string, which is alive for the whole call and is not
    // retained past it. The reference it returns is this call's to own.
    let policy = unsafe { SecPolicyCreateSSL(1, hostname.as_concrete_TypeRef()) };
    adopt(policy)
}

/// Take ownership of a reference [`server_ssl_policy`] got, reading NULL as
/// "no policy".
///
/// Separate from [`server_ssl_policy`] so a test can drive the NULL branch
/// without a restricted `trustd`.
fn adopt(policy: SecPolicyRef) -> Option<SecPolicy> {
    if policy.is_null() {
        return None;
    }
    // SAFETY: `SecPolicyCreateSSL` returns a reference whose count this
    // call owns, which is exactly what the create rule adopts, and the
    // reference was checked non-NULL above.
    Some(unsafe { SecPolicy::wrap_under_create_rule(policy) })
}

#[cfg(test)]
mod tests;
