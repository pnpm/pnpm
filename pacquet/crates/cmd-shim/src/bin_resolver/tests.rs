use super::{get_bins_from_package_manifest, pkg_owns_bin};
use crate::{capabilities::Host, path_util::lexical_normalize};
use pipe_trait::Pipe;
use serde_json::json;
use std::{
    fs::{create_dir_all, write as write_file},
    path::{Path, PathBuf},
};
use tempfile::tempdir;

#[test]
fn bin_as_string_uses_package_name() {
    let manifest = json!({"name": "foo", "bin": "cli.js"});
    let commands = get_bins_from_package_manifest::<Host>(&manifest, Path::new("/pkg/foo"));
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].name, "foo");
    assert_eq!(commands[0].path, Path::new("/pkg/foo/cli.js"));
}

#[test]
fn bin_as_string_strips_scope() {
    let manifest = json!({"name": "@scope/foo", "bin": "cli.js"});
    let commands = get_bins_from_package_manifest::<Host>(&manifest, Path::new("/pkg/foo"));
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].name, "foo");
}

#[test]
fn bin_as_object_keeps_keys_and_strips_scope() {
    let manifest = json!({
        "name": "tool",
        "bin": {
            "tool": "bin/tool.js",
            "@scope/extra": "bin/extra.js",
        },
    });
    let mut commands = get_bins_from_package_manifest::<Host>(&manifest, Path::new("/p"));
    commands.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(commands.len(), 2);
    assert_eq!(commands[0].name, "extra");
    assert_eq!(commands[1].name, "tool");
}

#[test]
fn rejects_unsafe_bin_names() {
    let manifest = json!({
        "name": "x",
        "bin": {
            "good-name": "ok.js",
            "../bad": "evil.js",
            "with space": "no.js",
            "$": "dollar.js",
        },
    });
    let mut names: Vec<_> = get_bins_from_package_manifest::<Host>(&manifest, Path::new("/p"))
        .into_iter()
        .map(|c| c.name)
        .collect();
    names.sort();
    assert_eq!(names, vec!["$".to_string(), "good-name".to_string()]);
}

#[test]
fn rejects_path_traversal_outside_package_root() {
    let manifest = json!({
        "name": "x",
        "bin": {"x": "../../../etc/passwd"},
    });
    let commands = get_bins_from_package_manifest::<Host>(&manifest, Path::new("/pkg/x"));
    assert!(commands.is_empty(), "must reject `..`-escapes from pkg root");
}

#[test]
fn no_bin_field_returns_empty() {
    let manifest = json!({"name": "x"});
    assert!(get_bins_from_package_manifest::<Host>(&manifest, Path::new("/p")).is_empty());
}

#[test]
fn pkg_owns_bin_default_rule() {
    assert!(pkg_owns_bin("foo", "foo"));
    assert!(!pkg_owns_bin("foo", "bar"));
}

#[test]
fn pkg_owns_bin_overrides() {
    assert!(pkg_owns_bin("npx", "npm"));
    assert!(pkg_owns_bin("pnpx", "pnpm"));
    assert!(pkg_owns_bin("pnpx", "@pnpm/exe"));
    assert!(!pkg_owns_bin("npx", "anything-else"));
}

/// Mirrors pnpm's "should allow $ as command name" test
/// (<https://github.com/pnpm/pnpm/blob/4750fd370c/bins/resolver/test/index.ts#L21-L36>).
/// `$` is the documented escape hatch for awkward bin names; it must
/// survive the URL-safe-name guard.
#[test]
fn dollar_is_allowed_as_command_name() {
    let manifest = json!({
        "name": "undollar",
        "version": "1.0.0",
        "bin": {"$": "./undollar.js"},
    });
    let commands = get_bins_from_package_manifest::<Host>(&manifest, Path::new("/p"));
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].name, "$");
}

/// Mirrors pnpm's "skip dangerous bin names" test
/// (<https://github.com/pnpm/pnpm/blob/4750fd370c/bins/resolver/test/index.ts#L74-L94>).
/// Path-traversal characters in the *key* must be filtered, not just the
/// value.
#[test]
fn skip_dangerous_bin_names() {
    let manifest = json!({
        "name": "foo",
        "version": "1.0.0",
        "bin": {
            "../bad": "./bad",
            r"..\bad": "./bad",
            "good": "./good",
            "~/bad": "./bad",
        },
    });
    let commands = get_bins_from_package_manifest::<Host>(&manifest, Path::new("/p"));
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].name, "good");
}

/// Mirrors pnpm's "skip dangerous bin locations" test
/// (<https://github.com/pnpm/pnpm/blob/4750fd370c/bins/resolver/test/index.ts#L96-L112>).
/// `../bad` in the *value* must be filtered by the `is_subdir` check.
#[test]
fn skip_dangerous_bin_locations() {
    let manifest = json!({
        "name": "foo",
        "version": "1.0.0",
        "bin": {
            "bad": "../bad",
            "good": "./good",
        },
    });
    let commands = get_bins_from_package_manifest::<Host>(&manifest, Path::new("/pkg/foo"));
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].name, "good");
}

/// Mirrors pnpm's "get bin from scoped bin name" test
/// (<https://github.com/pnpm/pnpm/blob/4750fd370c/bins/resolver/test/index.ts#L114-L130>).
/// A scoped key like `@foo/a` collapses to `a` before validation.
#[test]
fn scoped_bin_name_strips_scope_prefix() {
    let manifest = json!({
        "name": "@foo/a",
        "version": "1.0.0",
        "bin": {"@foo/a": "./a"},
    });
    let commands = get_bins_from_package_manifest::<Host>(&manifest, Path::new("/p"));
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].name, "a");
}

/// Mirrors pnpm's "skip scoped bin names with path traversal" test
/// (<https://github.com/pnpm/pnpm/blob/4750fd370c/bins/resolver/test/index.ts#L132-L148>).
/// After the scope strip, the resulting bare name still has to pass the
/// URL-safe guard. A `@scope/../etc/passwd` collapses to `../etc/passwd`
/// which must be rejected.
#[test]
fn skip_scoped_bin_names_with_path_traversal() {
    let manifest = json!({
        "name": "malicious",
        "version": "1.0.0",
        "bin": {
            "@scope/../../.npmrc": "./malicious.js",
            "@scope/../etc/passwd": "./evil.js",
            "@scope/legit": "./good.js",
        },
    });
    let commands = get_bins_from_package_manifest::<Host>(&manifest, Path::new("/p"));
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].name, "legit");
}

/// `bin` as a non-string non-object (number, array, null) is malformed.
/// Pacquet's port must return an empty list rather than panic, mirroring
/// pnpm's silent fall-through to the empty default.
#[test]
fn malformed_bin_type_returns_empty() {
    for shape in [json!(42), json!(["a", "b"]), json!(null), json!(true)] {
        let manifest = json!({"name": "x", "version": "1.0.0", "bin": shape});
        assert!(
            get_bins_from_package_manifest::<Host>(&manifest, Path::new("/p")).is_empty(),
            "malformed bin shape must be tolerated",
        );
    }
}

/// `bin` as a string requires the manifest to declare `name` (mirrors
/// pnpm's `INVALID_PACKAGE_NAME` guard). Pacquet returns an empty list
/// rather than throwing because the install pipeline would have already
/// surfaced a missing-name failure upstream.
#[test]
fn bin_string_with_missing_package_name_returns_empty() {
    let manifest = json!({"bin": "cli.js"});
    assert!(get_bins_from_package_manifest::<Host>(&manifest, Path::new("/p")).is_empty());
}

/// Object-form bin entries whose values aren't strings (number, null, etc.)
/// are silently skipped. Same defensive shape pnpm has for malformed
/// manifests.
#[test]
fn bin_object_with_non_string_value_is_skipped() {
    let manifest = json!({
        "name": "tool",
        "version": "1.0.0",
        "bin": {
            "good": "ok.js",
            "bad-num": 42,
            "bad-null": null,
        },
    });
    let commands = get_bins_from_package_manifest::<Host>(&manifest, Path::new("/p"));
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].name, "good");
}

/// Mirrors pnpm's "find all the bin files from a bin directory"
/// (<https://github.com/pnpm/pnpm/blob/4750fd370c/bins/resolver/test/index.ts#L37-L57>).
/// Every regular file under `directories.bin`, including files in
/// subdirectories, becomes a command.
#[test]
fn directories_bin_walks_files_recursively() {
    let tmp = tempdir().unwrap();
    let pkg = tmp.path().join("pkg");
    let bin_dir = pkg.join("bin-dir");
    create_dir_all(bin_dir.join("subdir")).unwrap();
    write_file(bin_dir.join("rootBin.js"), "").unwrap();
    write_file(bin_dir.join("subdir/subBin.js"), "").unwrap();

    let manifest = json!({
        "name": "bin-dir",
        "version": "1.0.0",
        "directories": {"bin": "bin-dir"},
    });
    let mut commands = get_bins_from_package_manifest::<Host>(&manifest, &pkg);
    commands.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(commands.len(), 2);
    assert_eq!(commands[0].name, "rootBin.js");
    assert_eq!(commands[0].path, bin_dir.join("rootBin.js"));
    assert_eq!(commands[1].name, "subBin.js");
    assert_eq!(commands[1].path, bin_dir.join("subdir/subBin.js"));
}

/// Mirrors pnpm's "skip directories.bin with path traversal"
/// (<https://github.com/pnpm/pnpm/blob/4750fd370c/bins/resolver/test/index.ts#L150-L170>).
/// `directories.bin: '../sibling'` must be rejected by the `is_subdir`
/// guard. The sibling directory is populated with a real file so a
/// regression that disables `is_subdir` would observably emit that file
/// as a command. Without the file the test would pass for the wrong
/// reason (empty dir, hence empty commands).
#[test]
fn directories_bin_rejects_path_traversal() {
    let tmp = tempdir().unwrap();
    let pkg = tmp.path().join("pkg");
    create_dir_all(&pkg).unwrap();

    // Sibling dir reachable via `../siblings` from the package root,
    // populated with a "smoking gun" file the resolver would emit if
    // it failed to reject the traversal.
    let siblings = tmp.path().join("siblings");
    create_dir_all(&siblings).unwrap();
    write_file(siblings.join("smoking-gun"), "").unwrap();

    let manifest = json!({
        "name": "malicious",
        "version": "1.0.0",
        "directories": {"bin": "../siblings"},
    });
    assert!(
        get_bins_from_package_manifest::<Host>(&manifest, &pkg).is_empty(),
        "is_subdir guard must reject `..`-escapes from the pkg root, even \
         when the resolved directory exists and has files",
    );
}

/// Mirrors pnpm's `path-traversal.test.ts`
/// (<https://github.com/pnpm/pnpm/blob/4750fd370c/bins/resolver/test/path-traversal.test.ts>).
/// A `directories.bin` value that resolves outside the package root via
/// `..` must yield no commands, even though the target dir exists and
/// has files in it.
#[test]
fn directories_bin_rejects_real_path_traversal() {
    let tmp = tempdir().unwrap();
    let secret_dir = tmp.path().join("secret");
    create_dir_all(&secret_dir).unwrap();
    write_file(secret_dir.join("secret.sh"), "echo secret").unwrap();

    let pkg = tmp.path().join("pkg");
    create_dir_all(&pkg).unwrap();

    // From `<tmp>/pkg`, `../secret` reaches `<tmp>/secret`. The
    // `is_subdir` guard must reject this even though the resolved path
    // exists and contains files.
    let manifest = json!({
        "name": "malicious",
        "version": "1.0.0",
        "directories": {"bin": "../secret"},
    });
    assert!(get_bins_from_package_manifest::<Host>(&manifest, &pkg).is_empty());
}

/// `directories.bin` pointing at a non-existent subdirectory must
/// degrade to an empty list (pnpm's `ENOENT` swallowing in `findFiles`).
#[test]
fn directories_bin_missing_directory_returns_empty() {
    let tmp = tempdir().unwrap();
    let pkg = tmp.path().join("pkg");
    create_dir_all(&pkg).unwrap();
    let manifest = json!({
        "name": "x",
        "version": "1.0.0",
        "directories": {"bin": "missing-dir"},
    });
    assert!(get_bins_from_package_manifest::<Host>(&manifest, &pkg).is_empty());
}

/// `directories.bin` filters out files whose basename fails the
/// URL-safe-name guard. Pin via a `..` filename: once the
/// path-traversal guard already passed (the dir was a real subdir),
/// a *file* inside it with an unsafe name still gets dropped.
#[test]
fn directories_bin_filters_unsafe_file_names() {
    let tmp = tempdir().unwrap();
    let pkg = tmp.path().join("pkg");
    let bin_dir = pkg.join("bin");
    create_dir_all(&bin_dir).unwrap();
    write_file(bin_dir.join("good"), "").unwrap();
    write_file(bin_dir.join("bad space"), "").unwrap();

    let manifest = json!({
        "name": "tool",
        "version": "1.0.0",
        "directories": {"bin": "bin"},
    });
    let mut commands = get_bins_from_package_manifest::<Host>(&manifest, &pkg);
    commands.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].name, "good");
}

/// Empty bin name returns false via the `is_empty` guard inside
/// `is_safe_bin_name`. Exercised via a `bin` object with an empty key.
#[test]
fn empty_bin_key_is_rejected() {
    let manifest = json!({
        "name": "x",
        "version": "1.0.0",
        "bin": {"": "ok.js", "good": "ok.js"},
    });
    let commands = get_bins_from_package_manifest::<Host>(&manifest, Path::new("/p"));
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].name, "good");
}

/// The relative names `.` and `..` survive the URL-safe guard's character
/// set (`.` is unescaped by `encodeURIComponent`) but resolve to the bin
/// directory itself or its parent when joined to a target dir, so
/// `is_safe_bin_name` must reject them. Scoped forms like `@scope/..`
/// collapse to `..` after the scope strip and must be rejected too.
#[test]
fn reserved_relative_bin_names_are_rejected() {
    let manifest = json!({
        "name": "malicious",
        "version": "1.0.0",
        "bin": {
            ".": "dot.js",
            "..": "dotdot.js",
            "@scope/..": "scoped-dotdot.js",
            "good": "good.js",
        },
    });
    let commands = get_bins_from_package_manifest::<Host>(&manifest, Path::new("/p"));
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].name, "good");
}

/// [`lexical_normalize`] drops `.` (`CurDir`) segments. This is a direct
/// test on the helper. The integration-style test below covers the same
/// arm via `directories.bin`, but a direct assertion makes the `CurDir`
/// branch visible to coverage tooling that can't see through inlined
/// call chains.
#[test]
fn lexical_normalize_drops_curdir_segments_directly() {
    assert_eq!(lexical_normalize(Path::new("a/./b")), PathBuf::from("a/b"));
    assert_eq!(lexical_normalize(Path::new("./a/b")), PathBuf::from("a/b"));
    assert_eq!(lexical_normalize(Path::new("a/b/.")), PathBuf::from("a/b"));
    assert_eq!(lexical_normalize(Path::new("./.")), PathBuf::new());
}

/// On an absolute path, a `..` that would escape the root is dropped
/// instead of being materialised as a literal `..` segment. Mirrors
/// Node's `path.resolve` (and therefore pnpm's `is-subdir`), where
/// `path.resolve("/a/../../a/bin.js") === "/a/bin.js"`.
///
/// Without this guard, `is_subdir("/a", "/a/../../a/bin.js")` would
/// reject the path as outside `/a` even though it resolves back
/// inside: `lexical_normalize` would have produced `/../a/bin.js`,
/// which fails `starts_with("/a")`.
#[test]
fn lexical_normalize_drops_excess_parent_dirs_on_absolute_paths() {
    assert_eq!(lexical_normalize(Path::new("/a/../../a/bin.js")), PathBuf::from("/a/bin.js"));
    assert_eq!(lexical_normalize(Path::new("/..")), PathBuf::from("/"));
    assert_eq!(lexical_normalize(Path::new("/../..")), PathBuf::from("/"));
}

/// `is_subdir` accepts a path that goes outside the package root and
/// comes back inside via `..`. Regression coverage for the lexical
/// normalisation bug where `/<pkg>/x/../../<pkg>/bin.js` was rejected
/// because the `..` past the root was materialised as a literal
/// `/..` segment. Mirrors pnpm's `path.resolve`-based `is-subdir`.
#[test]
fn directories_bin_accepts_excess_parent_dirs_that_resolve_inside_pkg() {
    let tmp = tempdir().unwrap();
    let pkg = tmp.path().join("pkg");
    let bin_dir = pkg.join("bin-dir");
    create_dir_all(&bin_dir).unwrap();
    // `x` must exist on disk so the walker can follow the literal
    // `<pkg>/x/../../pkg/bin-dir` path the resolver builds.
    create_dir_all(pkg.join("x")).unwrap();
    write_file(bin_dir.join("cli"), "").unwrap();

    // `<pkg>/x/../../<pkg-name>/bin-dir` resolves back inside `<pkg>`.
    let manifest = json!({
        "name": "tool",
        "version": "1.0.0",
        "directories": {"bin": "x/../../pkg/bin-dir"},
    });
    let commands = get_bins_from_package_manifest::<Host>(&manifest, &pkg);
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].name, "cli");
}

/// [`lexical_normalize`] `CurDir` branch drops `.` segments. Visible
/// via [`super::is_subdir`] accepting a target with embedded `./`
/// that resolves inside the package root.
#[test]
fn directories_bin_handles_curdir_in_relative_path() {
    let tmp = tempdir().unwrap();
    let pkg = tmp.path().join("pkg");
    let bin_dir = pkg.join("bin-dir");
    create_dir_all(&bin_dir).unwrap();
    write_file(bin_dir.join("cli"), "").unwrap();

    let manifest = json!({
        "name": "tool",
        "version": "1.0.0",
        "directories": {"bin": "./bin-dir"},
    });
    let commands = get_bins_from_package_manifest::<Host>(&manifest, &pkg);
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].name, "cli");
}

/// `commands_from_directories_bin` skips entries whose path doesn't
/// yield a usable file name. That covers the `path.file_name() == None`
/// and `to_str() == None` branches, both of which the real fs hardly
/// ever reaches (`file_name()` returns None only for paths ending in
/// `..`, and `to_str()` fails only on non-UTF-8 bytes which are rare on
/// Unix and impossible on Windows). A fake [`FsWalkFiles`] hands back one
/// such path so the `continue` arm gets exercised directly. The
/// regular `cli` entry alongside it confirms that the well-formed
/// path still flows through and emits a [`Command`](super::Command).
#[test]
fn directories_bin_skips_path_without_usable_file_name() {
    use crate::capabilities::FsWalkFiles;
    use std::io;

    struct EvilWalker;
    impl FsWalkFiles for EvilWalker {
        fn walk_files(_: &Path) -> io::Result<impl Iterator<Item = PathBuf>> {
            [
                // `file_name()` returns None for a path ending in `..`,
                // hitting the `let-else continue` branch.
                PathBuf::from("/pkg/bin/.."),
                // Well-formed sibling so we can assert the loop's
                // happy path still runs after the skip.
                PathBuf::from("/pkg/bin/cli"),
            ]
            .into_iter()
            .pipe(Ok)
        }
    }

    let manifest = json!({
        "name": "tool",
        "version": "1.0.0",
        "directories": {"bin": "bin"},
    });
    let commands = get_bins_from_package_manifest::<EvilWalker>(&manifest, Path::new("/pkg"));
    assert_eq!(commands.len(), 1, "the `..` entry must be skipped, not crashed on");
    assert_eq!(commands[0].name, "cli");
}

/// `bin` field takes precedence over `directories.bin` when both are
/// present. Mirrors upstream's order-of-checks in
/// `getBinsFromPackageManifest`.
#[test]
fn bin_field_takes_precedence_over_directories_bin() {
    let tmp = tempdir().unwrap();
    let pkg = tmp.path().join("pkg");
    let bin_dir = pkg.join("legacy-bin");
    create_dir_all(&bin_dir).unwrap();
    write_file(bin_dir.join("ignored.js"), "").unwrap();
    write_file(pkg.join("primary.js"), "").unwrap();

    let manifest = json!({
        "name": "tool",
        "version": "1.0.0",
        "bin": "primary.js",
        "directories": {"bin": "legacy-bin"},
    });
    let commands = get_bins_from_package_manifest::<Host>(&manifest, &pkg);
    assert_eq!(commands.len(), 1, "bin field wins, directories.bin is ignored");
    assert_eq!(commands[0].name, "tool");
}
