use super::{current_umask, read_umask_by_writing_it};

/// Pins that the published mask and the one read by writing it agree, so
/// the Linux fast path cannot answer differently from the fallback every
/// other target uses.
#[cfg(target_os = "linux")]
#[test]
fn the_published_mask_matches_the_one_read_by_writing_it() {
    assert_eq!(super::published_umask(), Some(read_umask_by_writing_it()));
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
    let status = "Name:\tpnpm\nUmask:\t0022\nState:\tR (running)\n";
    assert_eq!(super::parse_published_umask(status), Some(0o022));
}

#[cfg(target_os = "linux")]
#[test]
fn a_status_without_the_line_parses_as_absent() {
    assert_eq!(super::parse_published_umask("Name:\tpnpm\nState:\tR (running)\n"), None);
}

/// A pre-4.7 kernel prints no `Umask:` line, and a value that is not octal
/// would mean the field changed shape, so both read as absent and send the
/// caller to the fallback rather than to a wrong mask.
#[cfg(target_os = "linux")]
#[test]
fn a_non_octal_umask_line_parses_as_absent() {
    assert_eq!(super::parse_published_umask("Umask:\trw-r--r--\n"), None);
}
