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
