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

/// Compat entries must not inject `estree` — no such npm package exists,
/// the import that names it is type-only and satisfied by `@types/estree` —
/// nor a single-instance runtime like `typescript`, `react`, or `eslint`,
/// where a second copy in the graph breaks the tools that load it.
#[test]
fn compat_entries_never_inject_type_only_or_singleton_packages() {
    for target in ["estree", "typescript", "react", "eslint"] {
        for (selector, extension) in COMPAT_PACKAGE_EXTENSIONS.iter() {
            assert!(
                extension.dependencies.as_ref().is_none_or(|deps| !deps.contains_key(target)),
                "{selector} must not inject a {target} dependency",
            );
        }
    }
}
