use super::adopt;

/// Pins the branch a restricted `trustd` takes: it returns NULL, and the
/// caller has to read that as "no policy". Handing the NULL to
/// `core-foundation` instead raised the "Attempted to create a NULL object"
/// panic that `pnpr` reported on its first resolve
/// ([pnpm/pnpm#14461](https://github.com/pnpm/pnpm/issues/14461)).
///
/// Whether the framework hands out a policy at all is a property of the
/// host, so only the NULL branch can be pinned here; the other half is
/// covered where the policy is used.
#[test]
fn a_null_reference_reads_as_no_policy() {
    assert!(adopt(std::ptr::null_mut()).is_none());
}
