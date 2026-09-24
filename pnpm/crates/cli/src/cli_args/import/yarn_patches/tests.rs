use super::{RecordedPatch, YarnPatchSpecifier};
use pretty_assertions::assert_eq;

fn parse(specifier: &str) -> (String, String, Vec<String>) {
    let patch = YarnPatchSpecifier::parse(specifier).expect("a patch: specifier");
    (patch.specifier, patch.patch_key, patch.patch_paths)
}

fn owned(specifier: &str, patch_key: &str, patch_paths: &[&str]) -> (String, String, Vec<String>) {
    let patch_paths = patch_paths
        .iter()
        .map(ToString::to_string)
        .collect();
    (specifier.to_string(), patch_key.to_string(), patch_paths)
}

#[test]
fn parses_a_yarn_4_patch_of_a_registry_version() {
    assert_eq!(
        parse(
            "patch:jest-runtime@npm%3A29.7.0#~/.yarn/patches/jest-runtime-npm-29.7.0-120fa64128.patch"
        ),
        owned(
            "29.7.0",
            "jest-runtime@29.7.0",
            &["~/.yarn/patches/jest-runtime-npm-29.7.0-120fa64128.patch"],
        ),
    );
}

#[test]
fn parses_a_scoped_patch_with_yarn_3_parameters() {
    assert_eq!(
        parse(
            "patch:@scope/pkg@npm%3A%5E1.2.0#./.yarn/patches/pkg.patch::version=1.2.3&hash=abc&locator=root%40workspace%3A."
        ),
        owned("^1.2.0", "@scope/pkg@^1.2.0", &["./.yarn/patches/pkg.patch"]),
    );
}

#[test]
fn keys_an_aliased_patch_by_the_real_package() {
    assert_eq!(
        parse("patch:foo@npm%3A@scope/bar@1.0.0#~/foo.patch"),
        owned("npm:@scope/bar@1.0.0", "@scope/bar@1.0.0", &["~/foo.patch"]),
    );
}

#[test]
fn keys_a_non_registry_patch_by_name() {
    assert_eq!(
        parse("patch:foo@https%3A//example.com/foo.tgz#~/foo.patch"),
        owned("https://example.com/foo.tgz", "foo", &["~/foo.patch"]),
    );
}

#[test]
fn splits_several_patches_and_skips_yarn_builtins() {
    assert_eq!(
        parse(
            "patch:typescript@npm%3A5.0.0#optional!builtin<compat/typescript>&./a.patch&./b.patch"
        ),
        owned("5.0.0", "typescript@5.0.0", &["./a.patch", "./b.patch"]),
    );
    assert_eq!(
        parse("patch:resolve@npm%3A1.22.0#builtin<compat/resolve>"),
        owned("1.22.0", "resolve@1.22.0", &[]),
    );
}

#[test]
fn ignores_other_protocols() {
    assert_eq!(YarnPatchSpecifier::parse("npm:foo@1.0.0"), None);
    assert_eq!(YarnPatchSpecifier::parse("^1.0.0"), None);
    assert_eq!(YarnPatchSpecifier::parse("patch:foo"), None);
}

#[test]
fn records_a_patch_once_and_reports_a_conflicting_one() {
    let workspace_dir = std::path::Path::new("/workspace");
    let mut patched_dependencies =
        indexmap::IndexMap::from([("foo@1.0.0".to_string(), "./patches/foo.patch".to_string())]);
    let record = |file: &str, patched_dependencies: &mut indexmap::IndexMap<String, String>| {
        let file = workspace_dir.join(file);
        let patch =
            RecordedPatch { key: "foo@1.0.0".to_string(), file: &file, alias: "foo".to_string() };
        patch.record(patched_dependencies, workspace_dir).map(|dropped| dropped.warning())
    };

    assert_eq!(record("patches/foo.patch", &mut patched_dependencies), None);
    assert_eq!(
        record("packages/bar/foo.patch", &mut patched_dependencies),
        Some(
            r#"The Yarn patch packages/bar/foo.patch of "foo" was not applied, because "foo@1.0.0" already uses the patch ./patches/foo.patch."#
                .to_string()
        ),
    );
    assert_eq!(patched_dependencies["foo@1.0.0"], "./patches/foo.patch");
}
