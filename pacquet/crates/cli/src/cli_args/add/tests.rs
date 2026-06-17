use super::AddDependencyOptions;
use pacquet_package_manifest::DependencyGroup;
use pretty_assertions::assert_eq;

#[test]
fn dependency_options_to_dependency_groups() {
    use DependencyGroup::{Dev, Optional, Peer, Prod};
    let create_list = |opts: AddDependencyOptions| opts.dependency_groups().collect::<Vec<_>>();

    assert_eq!(
        create_list(AddDependencyOptions {
            save_prod: false,
            save_dev: false,
            save_optional: false,
            save_peer: false
        }),
        [Prod],
    );

    assert_eq!(
        create_list(AddDependencyOptions {
            save_prod: true,
            save_dev: false,
            save_optional: false,
            save_peer: false
        }),
        [Prod],
    );

    assert_eq!(
        create_list(AddDependencyOptions {
            save_prod: false,
            save_dev: true,
            save_optional: false,
            save_peer: false
        }),
        [Dev],
    );

    assert_eq!(
        create_list(AddDependencyOptions {
            save_prod: false,
            save_dev: false,
            save_optional: true,
            save_peer: false
        }),
        [Optional],
    );

    assert_eq!(
        create_list(AddDependencyOptions {
            save_prod: false,
            save_dev: false,
            save_optional: false,
            save_peer: true
        }),
        [Dev, Peer],
    );

    assert_eq!(
        create_list(AddDependencyOptions {
            save_prod: true,
            save_dev: false,
            save_optional: false,
            save_peer: true
        }),
        [Prod, Peer],
    );

    assert_eq!(
        create_list(AddDependencyOptions {
            save_prod: false,
            save_dev: true,
            save_optional: false,
            save_peer: true
        }),
        [Dev, Peer],
    );

    assert_eq!(
        create_list(AddDependencyOptions {
            save_prod: false,
            save_dev: false,
            save_optional: true,
            save_peer: true
        }),
        [Optional, Peer],
    );
}
