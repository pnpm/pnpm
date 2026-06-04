use super::RemoveDependencyOptions;
use pacquet_package_manifest::DependencyGroup;
use pretty_assertions::assert_eq;

#[test]
fn dependency_options_to_save_type() {
    use DependencyGroup::{Dev, Optional, Prod};
    let save_type = |opts: RemoveDependencyOptions| opts.save_type();

    // no flags -> any field
    assert_eq!(
        save_type(RemoveDependencyOptions {
            save_prod: false,
            save_dev: false,
            save_optional: false
        }),
        None,
    );

    // --save-prod -> dependencies
    assert_eq!(
        save_type(RemoveDependencyOptions {
            save_prod: true,
            save_dev: false,
            save_optional: false
        }),
        Some(Prod),
    );

    // --save-dev -> devDependencies
    assert_eq!(
        save_type(RemoveDependencyOptions {
            save_prod: false,
            save_dev: true,
            save_optional: false
        }),
        Some(Dev),
    );

    // --save-optional -> optionalDependencies
    assert_eq!(
        save_type(RemoveDependencyOptions {
            save_prod: false,
            save_dev: false,
            save_optional: true
        }),
        Some(Optional),
    );

    // --save-dev wins over --save-optional and --save-prod
    assert_eq!(
        save_type(RemoveDependencyOptions { save_prod: true, save_dev: true, save_optional: true }),
        Some(Dev),
    );
}
