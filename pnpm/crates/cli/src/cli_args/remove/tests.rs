use super::RemoveDependencyOptions;
use pacquet_package_manifest::DependencyGroup;
use pretty_assertions::assert_eq;

#[test]
fn dependency_options_to_save_type() {
    use DependencyGroup::{Dev, Optional, Prod};
    let save_type = |opts: RemoveDependencyOptions| opts.save_type();

    assert_eq!(
        save_type(RemoveDependencyOptions {
            save_prod: false,
            save_dev: false,
            save_optional: false
        }),
        None,
    );

    assert_eq!(
        save_type(RemoveDependencyOptions {
            save_prod: true,
            save_dev: false,
            save_optional: false
        }),
        Some(Prod),
    );

    assert_eq!(
        save_type(RemoveDependencyOptions {
            save_prod: false,
            save_dev: true,
            save_optional: false
        }),
        Some(Dev),
    );

    assert_eq!(
        save_type(RemoveDependencyOptions {
            save_prod: false,
            save_dev: false,
            save_optional: true
        }),
        Some(Optional),
    );

    assert_eq!(
        save_type(RemoveDependencyOptions { save_prod: true, save_dev: true, save_optional: true }),
        Some(Dev),
    );
}
