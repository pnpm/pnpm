use super::InstallContext;

/// A phase borrows the context by one pointer; the per-snapshot structs
/// that embed `&InstallContext` are sized on that.
#[test]
fn a_phase_borrows_the_context_by_one_pointer() {
    assert_eq!(
        size_of::<&InstallContext<'_>>(),
        size_of::<usize>(),
        "a phase must borrow the context, never embed it",
    );
}
