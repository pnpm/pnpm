//! A snapshot edge that only satisfies the package's own peer dependency is
//! not a dependency edge: <https://github.com/pnpm/pnpm/issues/13605>.

use super::{
    Include, all_dependencies, build_audit_path_index, lockfile_to_audit_request, parse_lockfile,
    path_info, vulnerable_names,
};
use pnpm_lockfile::Lockfile;

fn prod_only() -> Include {
    Include { dependencies: true, dev_dependencies: false, optional_dependencies: true }
}

/// `hookform` (prod) declares an optional peer on `valibot`, which is only
/// otherwise present as a devDependency.
fn optional_peer_satisfied_by_dev_dependency() -> Lockfile {
    parse_lockfile(
        "
lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      hookform:
        specifier: '^1.0.0'
        version: '1.0.0(valibot@1.2.0)'
    devDependencies:
      valibot:
        specifier: '^1.2.0'
        version: '1.2.0'

packages:

  hookform@1.0.0:
    resolution: {integrity: sha512-JYtls3hqi15fcx5GaSNL7SCTJ2MNmjrkHXg4FSpOA/grxK8KwyZ5bubHsCq8FXCkua6xhuaaBit+3b7+VZRfcA==}
    peerDependencies:
      valibot: ^1.0.0
    peerDependenciesMeta:
      valibot:
        optional: true

  valibot@1.2.0:
    resolution: {integrity: sha512-JYtls3hqi15fcx5GaSNL7SCTJ2MNmjrkHXg4FSpOA/grxK8KwyZ5bubHsCq8FXCkua6xhuaaBit+3b7+VZRfcA==}

snapshots:

  hookform@1.0.0(valibot@1.2.0):
    optionalDependencies:
      valibot: 1.2.0

  valibot@1.2.0: {}
",
    )
}

#[test]
fn lockfile_to_audit_request_excludes_optional_peer_satisfied_by_excluded_dev_dependency() {
    let lockfile = optional_peer_satisfied_by_dev_dependency();

    let full = lockfile_to_audit_request(&lockfile, None, all_dependencies());
    assert_eq!(full.request["hookform"], vec!["1.0.0"]);
    assert_eq!(full.request["valibot"], vec!["1.2.0"]);

    let prod_only = lockfile_to_audit_request(&lockfile, None, prod_only());
    assert_eq!(prod_only.request["hookform"], vec!["1.0.0"]);
    assert!(!prod_only.request.contains_key("valibot"));
}

/// A required-peer satisfaction edge lands in `dependencies` rather than
/// `optionalDependencies`, so it is followed regardless of the optional flag.
#[test]
fn lockfile_to_audit_request_excludes_required_peer_satisfied_by_excluded_dev_dependency() {
    let lockfile = parse_lockfile(
        "
lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      needs-react:
        specifier: '^1.0.0'
        version: '1.0.0(react@18.0.0)'
    devDependencies:
      react:
        specifier: '^18.0.0'
        version: '18.0.0'

packages:

  needs-react@1.0.0:
    resolution: {integrity: sha512-JYtls3hqi15fcx5GaSNL7SCTJ2MNmjrkHXg4FSpOA/grxK8KwyZ5bubHsCq8FXCkua6xhuaaBit+3b7+VZRfcA==}
    peerDependencies:
      react: ^18.0.0

  react@18.0.0:
    resolution: {integrity: sha512-JYtls3hqi15fcx5GaSNL7SCTJ2MNmjrkHXg4FSpOA/grxK8KwyZ5bubHsCq8FXCkua6xhuaaBit+3b7+VZRfcA==}

snapshots:

  needs-react@1.0.0(react@18.0.0):
    dependencies:
      react: 18.0.0

  react@18.0.0: {}
",
    );

    let prod_only = lockfile_to_audit_request(&lockfile, None, prod_only());
    assert_eq!(prod_only.request["needs-react"], vec!["1.0.0"]);
    assert!(!prod_only.request.contains_key("react"));
}

#[test]
fn lockfile_to_audit_request_excludes_required_peer_satisfied_by_excluded_prod_dependency() {
    let lockfile = parse_lockfile(
        "
lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      ui-lib:
        specifier: '^2.0.0'
        version: '2.0.0'
    devDependencies:
      needs-ui-lib:
        specifier: '^1.0.0'
        version: '1.0.0(ui-lib@2.0.0)'

packages:

  needs-ui-lib@1.0.0:
    resolution: {integrity: sha512-JYtls3hqi15fcx5GaSNL7SCTJ2MNmjrkHXg4FSpOA/grxK8KwyZ5bubHsCq8FXCkua6xhuaaBit+3b7+VZRfcA==}
    peerDependencies:
      ui-lib: ^2.0.0

  ui-lib@2.0.0:
    resolution: {integrity: sha512-JYtls3hqi15fcx5GaSNL7SCTJ2MNmjrkHXg4FSpOA/grxK8KwyZ5bubHsCq8FXCkua6xhuaaBit+3b7+VZRfcA==}

snapshots:

  needs-ui-lib@1.0.0(ui-lib@2.0.0):
    dependencies:
      ui-lib: 2.0.0

  ui-lib@2.0.0: {}
",
    );

    let full = lockfile_to_audit_request(&lockfile, None, all_dependencies());
    assert_eq!(full.request["needs-ui-lib"], vec!["1.0.0"]);
    assert_eq!(full.request["ui-lib"], vec!["2.0.0"]);

    let dev_only = lockfile_to_audit_request(
        &lockfile,
        None,
        Include { dependencies: false, dev_dependencies: true, optional_dependencies: false },
    );
    assert_eq!(dev_only.request["needs-ui-lib"], vec!["1.0.0"]);
    assert!(!dev_only.request.contains_key("ui-lib"));
}

#[test]
fn build_audit_path_index_classifies_peer_satisfied_by_dev_dependency_as_dev_not_optional() {
    let lockfile = optional_peer_satisfied_by_dev_dependency();

    let index = build_audit_path_index(
        &lockfile,
        None,
        &vulnerable_names(&["valibot"]),
        all_dependencies(),
    );

    let info = path_info(&index, "valibot", "1.2.0");
    assert_eq!(info.paths, vec![".>valibot"]);
    assert!(info.dev);
    assert!(!info.optional);
}

#[test]
fn lockfile_to_audit_request_excludes_optional_peer_satisfied_by_excluded_prod_dependency() {
    let lockfile = parse_lockfile(
        "
lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      helper-lib:
        specifier: '^1.0.0'
        version: '1.0.0'
    devDependencies:
      dev-tool:
        specifier: '^1.0.0'
        version: '1.0.0(helper-lib@1.0.0)'

packages:

  dev-tool@1.0.0:
    resolution: {integrity: sha512-JYtls3hqi15fcx5GaSNL7SCTJ2MNmjrkHXg4FSpOA/grxK8KwyZ5bubHsCq8FXCkua6xhuaaBit+3b7+VZRfcA==}
    peerDependencies:
      helper-lib: ^1.0.0
    peerDependenciesMeta:
      helper-lib:
        optional: true

  helper-lib@1.0.0:
    resolution: {integrity: sha512-JYtls3hqi15fcx5GaSNL7SCTJ2MNmjrkHXg4FSpOA/grxK8KwyZ5bubHsCq8FXCkua6xhuaaBit+3b7+VZRfcA==}

snapshots:

  dev-tool@1.0.0(helper-lib@1.0.0):
    optionalDependencies:
      helper-lib: 1.0.0

  helper-lib@1.0.0: {}
",
    );

    let full = lockfile_to_audit_request(&lockfile, None, all_dependencies());
    assert_eq!(full.request["dev-tool"], vec!["1.0.0"]);
    assert_eq!(full.request["helper-lib"], vec!["1.0.0"]);

    let dev_only = lockfile_to_audit_request(
        &lockfile,
        None,
        Include { dependencies: false, dev_dependencies: true, optional_dependencies: false },
    );
    assert_eq!(dev_only.request["dev-tool"], vec!["1.0.0"]);
    assert!(!dev_only.request.contains_key("helper-lib"));
}

/// `needs-peer-a` (prod) has a required peer on `peer-a` that no importer
/// declares, so `autoInstallPeers` installed it.
fn auto_installed_peer() -> Lockfile {
    parse_lockfile(
        "
lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      needs-peer-a:
        specifier: '^1.0.0'
        version: '1.0.0(peer-a@1.0.0)'
    devDependencies:
      dev-tool:
        specifier: '^1.0.0'
        version: '1.0.0'

packages:

  needs-peer-a@1.0.0:
    resolution: {integrity: sha512-JYtls3hqi15fcx5GaSNL7SCTJ2MNmjrkHXg4FSpOA/grxK8KwyZ5bubHsCq8FXCkua6xhuaaBit+3b7+VZRfcA==}
    peerDependencies:
      peer-a: ^1.0.0

  peer-a@1.0.0:
    resolution: {integrity: sha512-JYtls3hqi15fcx5GaSNL7SCTJ2MNmjrkHXg4FSpOA/grxK8KwyZ5bubHsCq8FXCkua6xhuaaBit+3b7+VZRfcA==}

  peer-a-dep@1.0.0:
    resolution: {integrity: sha512-JYtls3hqi15fcx5GaSNL7SCTJ2MNmjrkHXg4FSpOA/grxK8KwyZ5bubHsCq8FXCkua6xhuaaBit+3b7+VZRfcA==}

  dev-tool@1.0.0:
    resolution: {integrity: sha512-JYtls3hqi15fcx5GaSNL7SCTJ2MNmjrkHXg4FSpOA/grxK8KwyZ5bubHsCq8FXCkua6xhuaaBit+3b7+VZRfcA==}

snapshots:

  needs-peer-a@1.0.0(peer-a@1.0.0):
    dependencies:
      peer-a: 1.0.0

  peer-a@1.0.0:
    dependencies:
      peer-a-dep: 1.0.0

  peer-a-dep@1.0.0: {}

  dev-tool@1.0.0: {}
",
    )
}

#[test]
fn lockfile_to_audit_request_keeps_auto_installed_peer() {
    let lockfile = auto_installed_peer();

    let full = lockfile_to_audit_request(&lockfile, None, all_dependencies());
    assert_eq!(full.request["peer-a"], vec!["1.0.0"]);
    assert_eq!(full.request["peer-a-dep"], vec!["1.0.0"]);
    assert_eq!(full.total_dependencies, 4);
    assert_eq!(full.dependencies, 3);
    assert_eq!(full.dev_dependencies, 1);

    let prod_only = lockfile_to_audit_request(&lockfile, None, prod_only());
    assert_eq!(prod_only.request["peer-a"], vec!["1.0.0"]);
    assert_eq!(prod_only.request["peer-a-dep"], vec!["1.0.0"]);
    assert!(!prod_only.request.contains_key("dev-tool"));
}

#[test]
fn build_audit_path_index_records_path_of_auto_installed_peer() {
    let lockfile = auto_installed_peer();

    let index =
        build_audit_path_index(&lockfile, None, &vulnerable_names(&["peer-a"]), all_dependencies());

    let info = path_info(&index, "peer-a", "1.0.0");
    assert_eq!(info.paths, vec![".>needs-peer-a>peer-a"]);
    assert!(!info.dev);
    assert!(!info.optional);
}

#[test]
fn lockfile_to_audit_request_keeps_auto_installed_peer_another_importer_lists_as_dev_dependency() {
    let lockfile = parse_lockfile(
        "
lockfileVersion: '9.0'

importers:

  pkg-a:
    devDependencies:
      typescript:
        specifier: 5.4.5
        version: 5.4.5

  pkg-b:
    dependencies:
      needs-ts:
        specifier: '^1.0.0'
        version: '1.0.0(typescript@5.4.5)'

packages:

  needs-ts@1.0.0:
    resolution: {integrity: sha512-JYtls3hqi15fcx5GaSNL7SCTJ2MNmjrkHXg4FSpOA/grxK8KwyZ5bubHsCq8FXCkua6xhuaaBit+3b7+VZRfcA==}
    peerDependencies:
      typescript: ^5.0.0

  typescript@5.4.5:
    resolution: {integrity: sha512-JYtls3hqi15fcx5GaSNL7SCTJ2MNmjrkHXg4FSpOA/grxK8KwyZ5bubHsCq8FXCkua6xhuaaBit+3b7+VZRfcA==}

snapshots:

  needs-ts@1.0.0(typescript@5.4.5):
    dependencies:
      typescript: 5.4.5

  typescript@5.4.5: {}
",
    );

    let prod_only = lockfile_to_audit_request(&lockfile, None, prod_only());
    assert_eq!(prod_only.request["needs-ts"], vec!["1.0.0"]);
    assert_eq!(prod_only.request["typescript"], vec!["5.4.5"]);
}

#[test]
fn lockfile_to_audit_request_keeps_auto_installed_peer_a_dev_dependency_depends_on() {
    let lockfile = parse_lockfile(
        "
lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      needs-peer-a:
        specifier: '^1.0.0'
        version: '1.0.0(peer-a@1.0.0)'
    devDependencies:
      dev-tool:
        specifier: '^1.0.0'
        version: '1.0.0'

packages:

  needs-peer-a@1.0.0:
    resolution: {integrity: sha512-JYtls3hqi15fcx5GaSNL7SCTJ2MNmjrkHXg4FSpOA/grxK8KwyZ5bubHsCq8FXCkua6xhuaaBit+3b7+VZRfcA==}
    peerDependencies:
      peer-a: ^1.0.0

  dev-tool@1.0.0:
    resolution: {integrity: sha512-JYtls3hqi15fcx5GaSNL7SCTJ2MNmjrkHXg4FSpOA/grxK8KwyZ5bubHsCq8FXCkua6xhuaaBit+3b7+VZRfcA==}

  peer-a@1.0.0:
    resolution: {integrity: sha512-JYtls3hqi15fcx5GaSNL7SCTJ2MNmjrkHXg4FSpOA/grxK8KwyZ5bubHsCq8FXCkua6xhuaaBit+3b7+VZRfcA==}

snapshots:

  needs-peer-a@1.0.0(peer-a@1.0.0):
    dependencies:
      peer-a: 1.0.0

  dev-tool@1.0.0:
    dependencies:
      peer-a: 1.0.0

  peer-a@1.0.0: {}
",
    );

    let prod_only = lockfile_to_audit_request(&lockfile, None, prod_only());
    assert_eq!(prod_only.request["needs-peer-a"], vec!["1.0.0"]);
    assert_eq!(prod_only.request["peer-a"], vec!["1.0.0"]);
}

#[test]
fn lockfile_to_audit_request_keeps_peer_another_importer_reaches_without_listing() {
    let lockfile = parse_lockfile(
        "
lockfileVersion: '9.0'

importers:

  pkg-a:
    dependencies:
      needs-ts:
        specifier: '^1.0.0'
        version: '1.0.0(typescript@5.4.5)'
    devDependencies:
      typescript:
        specifier: 5.4.5
        version: 5.4.5

  pkg-b:
    dependencies:
      needs-ts:
        specifier: '^1.0.0'
        version: '1.0.0(typescript@5.4.5)'

packages:

  needs-ts@1.0.0:
    resolution: {integrity: sha512-JYtls3hqi15fcx5GaSNL7SCTJ2MNmjrkHXg4FSpOA/grxK8KwyZ5bubHsCq8FXCkua6xhuaaBit+3b7+VZRfcA==}
    peerDependencies:
      typescript: ^5.0.0

  typescript@5.4.5:
    resolution: {integrity: sha512-JYtls3hqi15fcx5GaSNL7SCTJ2MNmjrkHXg4FSpOA/grxK8KwyZ5bubHsCq8FXCkua6xhuaaBit+3b7+VZRfcA==}

snapshots:

  needs-ts@1.0.0(typescript@5.4.5):
    dependencies:
      typescript: 5.4.5

  typescript@5.4.5: {}
",
    );

    let prod_only = lockfile_to_audit_request(&lockfile, None, prod_only());
    assert_eq!(prod_only.request["needs-ts"], vec!["1.0.0"]);
    assert_eq!(prod_only.request["typescript"], vec!["5.4.5"]);
}

#[test]
fn lockfile_to_audit_request_keeps_peer_only_the_workspace_root_lists() {
    let lockfile = parse_lockfile(
        "
lockfileVersion: '9.0'

importers:

  .:
    devDependencies:
      typescript:
        specifier: 5.4.5
        version: 5.4.5

  pkg-b:
    dependencies:
      needs-ts:
        specifier: '^1.0.0'
        version: '1.0.0(typescript@5.4.5)'

packages:

  needs-ts@1.0.0:
    resolution: {integrity: sha512-JYtls3hqi15fcx5GaSNL7SCTJ2MNmjrkHXg4FSpOA/grxK8KwyZ5bubHsCq8FXCkua6xhuaaBit+3b7+VZRfcA==}
    peerDependencies:
      typescript: ^5.0.0

  typescript@5.4.5:
    resolution: {integrity: sha512-JYtls3hqi15fcx5GaSNL7SCTJ2MNmjrkHXg4FSpOA/grxK8KwyZ5bubHsCq8FXCkua6xhuaaBit+3b7+VZRfcA==}

snapshots:

  needs-ts@1.0.0(typescript@5.4.5):
    dependencies:
      typescript: 5.4.5

  typescript@5.4.5: {}
",
    );

    let prod_only = lockfile_to_audit_request(&lockfile, None, prod_only());
    assert_eq!(prod_only.request["needs-ts"], vec!["1.0.0"]);
    assert_eq!(prod_only.request["typescript"], vec!["5.4.5"]);
}
