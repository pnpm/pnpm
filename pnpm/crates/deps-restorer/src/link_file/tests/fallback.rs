//! Every failure [`LinkFailure`] knows, against every import method, from
//! the link call that method makes.

use super::{
    super::{
        AUTO_FIRST_TIER, ImportState, LINK_STATE_CLONE, LINK_STATE_COPY, LINK_STATE_HARDLINK,
        LOG_FLAG_CLONE, LOG_FLAG_COPY, LOG_FLAG_HARDLINK, next_auto_tier,
    },
    Faults, Faulty, STORE_ENTRY, write_source,
};
use pnpm_config::PackageImportMethod;
use pnpm_fs::{LinkFailure, StoreFile};
use pnpm_reporter::{PackageImportMethod as WireImportMethod, SilentReporter};
use pretty_assertions::assert_eq;
use std::{
    fs, io,
    path::Path,
    sync::atomic::{AtomicU8, Ordering},
};
use tempfile::tempdir;

/// What a method does when its link call fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Expect {
    /// The error surfaces and the method's tier stays.
    Fails,
    /// The tier is retired and the next method materializes the file.
    NextMethod,
    /// The file is copied and the tier stays.
    CopiesFile,
}

/// `auto` and `clone-or-copy` move on from a filesystem that cannot link,
/// copy one file that has no names left, and surface a path error.
fn ladder(failure: LinkFailure) -> Expect {
    match failure {
        LinkFailure::Path => Expect::Fails,
        LinkFailure::TooManyLinks => Expect::CopiesFile,
        LinkFailure::CrossDevice | LinkFailure::Denied | LinkFailure::Unsupported => {
            Expect::NextMethod
        }
    }
}

/// `hardlink` copies only what costs one file: a filesystem that refuses
/// links would copy every package, which is what choosing `hardlink`
/// rules out.
fn pinned_hardlink(failure: LinkFailure) -> Expect {
    match failure {
        LinkFailure::CrossDevice | LinkFailure::TooManyLinks => Expect::CopiesFile,
        LinkFailure::Path | LinkFailure::Denied | LinkFailure::Unsupported => Expect::Fails,
    }
}

/// `clone` never copies.
fn pinned_clone(_: LinkFailure) -> Expect {
    Expect::Fails
}

/// Which link call fails.
#[derive(Debug, Clone, Copy)]
enum Link {
    HardLink,
    Reflink,
}

struct Case {
    name: &'static str,
    method: PackageImportMethod,
    /// The tier a ladder starts at, or `None` for a pinned case.
    tier: Option<u8>,
    link: Link,
    expect: fn(LinkFailure) -> Expect,
}

const CASES: [Case; 5] = [
    Case {
        name: "auto from the clone tier",
        method: PackageImportMethod::Auto,
        tier: Some(LINK_STATE_CLONE),
        link: Link::Reflink,
        expect: ladder,
    },
    Case {
        name: "auto from the hardlink tier",
        method: PackageImportMethod::Auto,
        tier: Some(LINK_STATE_HARDLINK),
        link: Link::HardLink,
        expect: ladder,
    },
    Case {
        name: "clone-or-copy",
        method: PackageImportMethod::CloneOrCopy,
        tier: Some(LINK_STATE_CLONE),
        link: Link::Reflink,
        expect: ladder,
    },
    Case {
        name: "hardlink",
        method: PackageImportMethod::Hardlink,
        tier: None,
        link: Link::HardLink,
        expect: pinned_hardlink,
    },
    Case {
        name: "clone",
        method: PackageImportMethod::Clone,
        tier: None,
        link: Link::Reflink,
        expect: pinned_clone,
    },
];

fn wire_method(tier: u8) -> WireImportMethod {
    match tier {
        LINK_STATE_CLONE => WireImportMethod::Clone,
        LINK_STATE_HARDLINK => WireImportMethod::Hardlink,
        _ => WireImportMethod::Copy,
    }
}

fn log_flag(method: WireImportMethod) -> u8 {
    match method {
        WireImportMethod::Clone => LOG_FLAG_CLONE,
        WireImportMethod::Hardlink => LOG_FLAG_HARDLINK,
        WireImportMethod::Copy => LOG_FLAG_COPY,
    }
}

/// An import state whose ladders start at `tier`.
fn state_at(tier: u8) -> ImportState {
    ImportState { auto: AtomicU8::new(tier), clone_or_copy: AtomicU8::new(tier) }
}

/// The tier `method`'s ladder sits at.
fn tier_of(state: &ImportState, method: PackageImportMethod) -> u8 {
    match method {
        PackageImportMethod::CloneOrCopy => state.clone_or_copy.load(Ordering::Relaxed),
        _ => state.auto.load(Ordering::Relaxed),
    }
}

/// The tier `method`'s ladder retires `tier` to.
fn tier_after(method: PackageImportMethod, tier: u8) -> u8 {
    match method {
        PackageImportMethod::CloneOrCopy => LINK_STATE_COPY,
        _ => next_auto_tier(tier),
    }
}

/// A store entry at the mode the current umask wants, so every tier is
/// tried and lands the same mode.
fn store_entry(dir: &Path) -> StoreFile {
    let source = write_source(dir, STORE_ENTRY, b"store content\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&source, fs::Permissions::from_mode(desired_mode())).unwrap();
    }
    source
}

#[cfg(unix)]
fn desired_mode() -> u32 {
    pnpm_fs::file_mode::store_entry_mode(false, pnpm_fs::file_mode::current_umask())
}

/// What an error carries of the OS code that produced it.
fn error_code(error: &io::Error) -> (io::ErrorKind, Option<i32>) {
    (error.kind(), error.raw_os_error())
}

fn assert_materialized(source: &Path, target: &Path, context: &str) {
    assert_eq!(fs::read(target).unwrap(), fs::read(source).unwrap(), "{context}: content");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(target)
                .unwrap()
                .permissions()
                .mode()
                & 0o7777,
            desired_mode(),
            "{context}: mode",
        );
    }
}

fn assert_copied(source: &Path, target: &Path, context: &str) {
    assert_materialized(source, target, context);
    #[cfg(unix)]
    assert_ne!(super::inode(source), super::inode(target), "{context}: the copy has its own inode");
}

/// A failed link is answered as [`LinkFailure`] says for the method, and
/// retires the tier only when the whole filesystem is at fault.
#[test]
fn every_failure_under_every_method() {
    let tmp = tempdir().unwrap();
    let source = store_entry(tmp.path());
    for failure in LinkFailure::ALL {
        for case in &CASES {
            check_case(tmp.path(), &source, failure, case);
        }
    }
}

fn check_case(dir: &Path, source: &StoreFile, failure: LinkFailure, case: &Case) {
    let context = format!("{failure:?} under {}", case.name);
    let target = dir.join(format!("{failure:?}-{}", case.name));
    Faulty::set(match case.link {
        Link::HardLink => Faults { hard_link: Some(failure), ..Faults::default() },
        Link::Reflink => Faults { reflink: Some(failure), ..Faults::default() },
    });
    let tier = case.tier.unwrap_or(LINK_STATE_COPY);
    let state = state_at(tier);
    let logged = AtomicU8::new(0);

    let result = state.import::<SilentReporter, Faulty>(case.method, &logged, source, &target);

    let expect = (case.expect)(failure);
    let logged = logged.load(Ordering::Relaxed);
    let placed = Placed { source: &source.path, target: &target, logged, context: &context };
    match expect {
        Expect::Fails => placed.assert_failed(result, failure),
        Expect::NextMethod => {
            placed.assert_landed(result, wire_method(tier_after(case.method, tier)));
        }
        Expect::CopiesFile => placed.assert_copied(result),
    }
    let expected_tier = match expect {
        Expect::NextMethod => tier_after(case.method, tier),
        Expect::Fails | Expect::CopiesFile => tier,
    };
    assert_eq!(
        tier_of(&state, case.method),
        expected_tier,
        "{context}: the tier is retired only when the filesystem is at fault",
    );
}

/// What one import left behind: the paths, the logged methods, and the
/// case it ran under.
struct Placed<'a> {
    source: &'a Path,
    target: &'a Path,
    logged: u8,
    context: &'a str,
}

impl Placed<'_> {
    fn assert_failed(&self, result: io::Result<WireImportMethod>, failure: LinkFailure) {
        let context = self.context;
        let error = result.expect_err(context);
        assert_eq!(error_code(&error), error_code(&failure.sample_error()), "{context}: surfaces");
        assert!(!self.target.exists(), "{context}: nothing is written");
        assert_eq!(self.logged, 0, "{context}: no method is logged");
    }

    fn assert_landed(&self, result: io::Result<WireImportMethod>, landed: WireImportMethod) {
        let context = self.context;
        assert_eq!(result.expect(context), landed, "{context}");
        assert_materialized(self.source, self.target, context);
        assert_eq!(self.logged, log_flag(landed), "{context}");
    }

    fn assert_copied(&self, result: io::Result<WireImportMethod>) {
        let context = self.context;
        assert_eq!(result.expect(context), WireImportMethod::Copy, "{context}");
        assert_copied(self.source, self.target, context);
        assert_eq!(self.logged, LOG_FLAG_COPY, "{context}");
    }
}

/// With both link calls refusing the file, `auto` reaches the copy tier,
/// and what the copy then finds wrong is the error the caller sees. A
/// tier that only copied one file keeps its place.
#[test]
fn a_failed_copy_after_a_failed_link_surfaces_the_copy_error() {
    let tmp = tempdir().unwrap();
    let source = store_entry(tmp.path());
    for failure in LinkFailure::ALL {
        if ladder(failure) == Expect::Fails {
            continue;
        }
        let context = format!("{failure:?} then a full disk");
        let target = tmp
            .path()
            .join(format!("{failure:?}-copy-fails"));
        Faulty::set(Faults {
            hard_link: Some(failure),
            reflink: Some(failure),
            copy: Some(io::ErrorKind::StorageFull),
        });
        let state = state_at(AUTO_FIRST_TIER);

        let error = state
            .import::<SilentReporter, Faulty>(
                PackageImportMethod::Auto,
                &AtomicU8::new(0),
                &source,
                &target,
            )
            .expect_err(&context);

        assert_eq!(error.kind(), io::ErrorKind::StorageFull, "{context}");
        assert!(!target.exists(), "{context}: nothing is written");
        let expected_tier = match ladder(failure) {
            Expect::NextMethod => LINK_STATE_COPY,
            Expect::CopiesFile | Expect::Fails => AUTO_FIRST_TIER,
        };
        assert_eq!(state.auto.load(Ordering::Relaxed), expected_tier, "{context}");
    }
}

/// The copy tier has nothing to fall back to: its error surfaces and no
/// ladder moves.
#[test]
fn a_failed_copy_surfaces_under_every_method_that_copies() {
    let tmp = tempdir().unwrap();
    let source = store_entry(tmp.path());
    Faulty::set(Faults { copy: Some(io::ErrorKind::StorageFull), ..Faults::default() });
    for method in
        [PackageImportMethod::Copy, PackageImportMethod::Auto, PackageImportMethod::CloneOrCopy]
    {
        let target = tmp.path().join(format!("{method:?}"));
        let state = state_at(LINK_STATE_COPY);

        let error = state
            .import::<SilentReporter, Faulty>(method, &AtomicU8::new(0), &source, &target)
            .expect_err("the copy fails");

        assert_eq!(error.kind(), io::ErrorKind::StorageFull, "{method:?}");
        assert!(!target.exists(), "{method:?}: nothing is written");
        assert_eq!(tier_of(&state, method), LINK_STATE_COPY, "{method:?}");
    }
}
