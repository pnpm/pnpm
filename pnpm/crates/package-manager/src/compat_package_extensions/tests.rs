use super::COMPAT_PACKAGE_EXTENSIONS;

#[test]
fn includes_pnpm_specific_compat_entries() {
    let angular_build = COMPAT_PACKAGE_EXTENSIONS
        .get("@angular/build@*")
        .expect("@angular/build compat entry present");
    assert_eq!(
        angular_build.dependencies.as_ref().and_then(|deps| deps.get("tslib")),
        Some(&"^2.3.0".to_string()),
    );
    let legacy_nuxt_vite_builder = COMPAT_PACKAGE_EXTENSIONS
        .get("@nuxt/vite-builder@>=4.0.0 <4.5.0")
        .expect("legacy @nuxt/vite-builder compat entry present");
    assert_eq!(
        legacy_nuxt_vite_builder.dependencies.as_ref().and_then(|deps| deps.get("unplugin")),
        Some(&"^2.3.5".to_string()),
    );
    let nuxt_vite_builder = COMPAT_PACKAGE_EXTENSIONS
        .get("@nuxt/vite-builder@>=4.5.0")
        .expect("@nuxt/vite-builder compat entry present");
    assert_eq!(
        nuxt_vite_builder.dependencies.as_ref().and_then(|deps| deps.get("unplugin")),
        Some(&"^3.3.0".to_string()),
    );
}

/// Verify the phantom-dependency entries detected by static analysis of
/// the top-500 most-downloaded npm packages. Each entry declares a
/// dependency the package imports but doesn't declare in its package.json -
/// a phantom that resolves under npm's flat layout but breaks under a
/// strict resolver. Only phantoms that are installable packages belong
/// here: a type-only import satisfied by a `@types/*` package (like
/// eslint's `estree`) names no npm package at all, and an entry for it
/// makes every fresh resolve of the extended package fail with a 404.
#[test]
fn includes_phantom_dependency_entries() {
    let assert_dep = |selector: &str, target: &str| {
        let entry = COMPAT_PACKAGE_EXTENSIONS
            .get(selector)
            .unwrap_or_else(|| panic!("phantom entry {selector} missing"));
        let dep = entry
            .dependencies
            .as_ref()
            .and_then(|deps| deps.get(target))
            .unwrap_or_else(|| panic!("phantom target {target} missing from {selector}"));
        assert_eq!(dep, "*");
    };

    assert_dep("@eslint/core@*", "json-schema");
    assert_dep("@eslint/eslintrc@*", "eslint");
    assert_dep("@jest/types@*", "istanbul-lib-coverage");
    assert_dep("@jest/types@*", "istanbul-reports");
    assert_dep("@jest/types@*", "yargs");
    assert_dep("@types/babel__core@*", "@babel/generator");
    assert_dep("@types/babel__core@*", "@babel/template");
    assert_dep("@types/babel__core@*", "@babel/traverse");
    assert_dep("@types/react-dom@*", "react");
    assert_dep("@types/yargs@*", "yargs-parser");
    assert_dep("@typescript-eslint/types@*", "typescript");
    assert_dep("es-abstract@*", "for-each");
}

/// The phantom findings must never name a target that is not an
/// installable npm package. `estree` is a type-only import whose types
/// live in `@types/estree` — there is no runtime package behind it, so
/// an entry targeting it makes every fresh resolve of the extended
/// package fail with a 404.
#[test]
fn phantom_entries_never_target_estree() {
    for (selector, extension) in COMPAT_PACKAGE_EXTENSIONS.iter() {
        assert!(
            extension.dependencies.as_ref().is_none_or(|deps| !deps.contains_key("estree")),
            "{selector} must not depend on the nonexistent estree package",
        );
    }
}
