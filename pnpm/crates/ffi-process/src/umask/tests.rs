use super::{current_umask, read_umask_by_writing_it};
#[cfg(target_os = "linux")]
use text_block_macros::text_block_fnl;

/// Pins that the published mask and the one read by writing it agree, so
/// the Linux fast path cannot answer differently from the fallback every
/// other target uses.
#[cfg(target_os = "linux")]
#[test]
fn the_published_mask_matches_the_one_read_by_writing_it() {
    let by_writing = read_umask_by_writing_it();
    match super::published_umask() {
        Some(published) => assert_eq!(published, by_writing),
        // No `procfs` to publish it, so what the read must hold to is the
        // fallback's answer. Asserted rather than skipped, so the case is
        // covered on a host that has it and on one that does not.
        None => assert_eq!(current_umask(), by_writing),
    }
}

/// Pins that reading the mask leaves it as it was, which is what keeps a
/// concurrent file creation from seeing an unmasked mode after the read.
#[test]
fn reading_the_mask_restores_it() {
    let before = read_umask_by_writing_it();
    let _ = current_umask();
    assert_eq!(read_umask_by_writing_it(), before);
}

#[cfg(target_os = "linux")]
#[test]
fn an_octal_umask_line_parses() {
    let status = text_block_fnl! {
        "Name:\tpnpm"
        "Umask:\t0022"
        "State:\tR (running)"
    };
    assert_eq!(super::parse_published_umask(status), Some(0o022));
}

#[cfg(target_os = "linux")]
#[test]
fn a_status_without_the_line_parses_as_absent() {
    let status = text_block_fnl! {
        "Name:\tpnpm"
        "State:\tR (running)"
    };
    assert_eq!(super::parse_published_umask(status), None);
}

/// A pre-4.7 kernel prints no `Umask:` line, and a value that is not octal
/// would mean the field changed shape, so both read as absent and send the
/// caller to the fallback rather than to a wrong mask.
#[cfg(target_os = "linux")]
#[test]
fn a_non_octal_umask_line_parses_as_absent() {
    let status = text_block_fnl! { "Umask:\trw-r--r--" };
    assert_eq!(super::parse_published_umask(status), None);
}
