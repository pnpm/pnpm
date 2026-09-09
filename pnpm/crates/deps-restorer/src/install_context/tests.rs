use super::InstallContext;

/// The context exists so a phase can hold one pointer where it used
/// to hold eight fields. If it ever stops being pointer-sized to
/// borrow, the per-snapshot structs that embed `&InstallContext`
/// stop getting smaller.
#[test]
fn a_phase_borrows_the_context_by_one_pointer() {
    assert_eq!(
        size_of::<&InstallContext<'_>>(),
        size_of::<usize>(),
        "a phase must borrow the context, never embed it",
    );
}
