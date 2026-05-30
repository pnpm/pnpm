use super::AddDependencyOptions;
use pacquet_package_manifest::DependencyGroup;
use pretty_assertions::assert_eq;

#[test]
fn dependency_options_to_dependency_groups() {
    use DependencyGroup::{Dev, Optional, Peer, Prod};
    let create_list = |opts: AddDependencyOptions| opts.dependency_groups().collect::<Vec<_>>();

    // no flags -> prod
    assert_eq!(
        create_list(AddDependencyOptions {
            save_prod: false,
            save_dev: false,
            save_optional: false,
            save_peer: false
        }),
        [Prod],
    );

    // --save-prod -> prod
    assert_eq!(
        create_list(AddDependencyOptions {
            save_prod: true,
            save_dev: false,
            save_optional: false,
            save_peer: false
        }),
        [Prod],
    );

    // --save-dev -> dev
    assert_eq!(
        create_list(AddDependencyOptions {
            save_prod: false,
            save_dev: true,
            save_optional: false,
            save_peer: false
        }),
        [Dev],
    );

    // --save-optional -> optional
    assert_eq!(
        create_list(AddDependencyOptions {
            save_prod: false,
            save_dev: false,
            save_optional: true,
            save_peer: false
        }),
        [Optional],
    );

    // --save-peer -> dev + peer
    assert_eq!(
        create_list(AddDependencyOptions {
            save_prod: false,
            save_dev: false,
            save_optional: false,
            save_peer: true
        }),
        [Dev, Peer],
    );

    // --save-prod --save-peer -> prod + peer
    assert_eq!(
        create_list(AddDependencyOptions {
            save_prod: true,
            save_dev: false,
            save_optional: false,
            save_peer: true
        }),
        [Prod, Peer],
    );

    // --save-dev --save-peer -> dev + peer
    assert_eq!(
        create_list(AddDependencyOptions {
            save_prod: false,
            save_dev: true,
            save_optional: false,
            save_peer: true
        }),
        [Dev, Peer],
    );

    // --save-optional --save-peer -> optional + peer
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
