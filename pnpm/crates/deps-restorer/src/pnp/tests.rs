use super::render_pnp_loader;

#[test]
fn template_markers_inside_values_are_not_replaced() {
    let loader =
        render_pnp_loader("registry __PNPM_MODULES_DIR__", "modules __PNPM_PACKAGE_REGISTRY__");

    assert_eq!(loader.matches("registry __PNPM_MODULES_DIR__").count(), 1);
    assert_eq!(loader.matches("modules __PNPM_PACKAGE_REGISTRY__").count(), 1);
}
