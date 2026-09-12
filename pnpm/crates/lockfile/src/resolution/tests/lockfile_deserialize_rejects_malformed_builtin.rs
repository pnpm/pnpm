use super::{
    ARTIFACTORY_REGISTRY, LockfileFormOptions, LockfileResolution, RegistryResolution,
    RegistryServerType, SHA512, TarballResolution, artifactory_form, assert_eq, integrity,
    text_block, undeclared_form,
};

/// A malformed built-in resolution must stay a parse error: the custom
/// passthrough accepts only non-built-in `type` tags, so a `git` entry
/// missing its `commit` cannot silently reclassify as custom and dodge
/// the strict built-in shape checks.
#[test]
fn deserialize_rejects_malformed_builtin_resolution() {
    let yaml = text_block! {
        "type: git"
        "repo: https://github.com/user/repo.git"
    };
    let received = serde_saphyr::from_str::<LockfileResolution>(yaml);
    dbg!(&received);
    assert!(received.is_err(), "a git resolution without a commit must not parse");
}

#[test]
fn to_lockfile_form_drops_the_artifactory_url_of_a_scoped_package() {
    let tarball = format!("{ARTIFACTORY_REGISTRY}@acme/widget/-/@acme/widget-1.2.3.tgz");
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball,
        integrity: Some(integrity(SHA512)),
        revision: None,
        git_hosted: None,
        path: None,
    });
    let actual =
        resolution.to_lockfile_form("@acme/widget", "1.2.3", artifactory_form(false)).unwrap();
    assert_eq!(
        actual,
        LockfileResolution::Registry(RegistryResolution {
            integrity: integrity(SHA512),
            revision: None
        }),
    );
}

/// Under the Artifactory layout the npm-layout URL is the one pnpm cannot
/// rebuild, so it has to survive verbatim — the inverse of the default.
#[test]
fn to_lockfile_form_keeps_the_npm_layout_url_on_an_artifactory_registry() {
    let tarball = format!("{ARTIFACTORY_REGISTRY}@acme/widget/-/widget-1.2.3.tgz");
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball,
        integrity: Some(integrity(SHA512)),
        revision: None,
        git_hosted: None,
        path: None,
    });
    let actual =
        resolution.to_lockfile_form("@acme/widget", "1.2.3", artifactory_form(false)).unwrap();
    assert_eq!(actual, resolution);
}

#[test]
fn to_lockfile_form_keeps_the_artifactory_url_on_a_registry_left_on_the_npm_layout() {
    let tarball = format!("{ARTIFACTORY_REGISTRY}@acme/widget/-/@acme/widget-1.2.3.tgz");
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball,
        integrity: Some(integrity(SHA512)),
        revision: None,
        git_hosted: None,
        path: None,
    });
    let actual = resolution
        .to_lockfile_form("@acme/widget", "1.2.3", undeclared_form(ARTIFACTORY_REGISTRY, false))
        .unwrap();
    assert_eq!(actual, resolution);
}

#[test]
fn to_lockfile_form_keeps_the_artifactory_url_when_include_tarball_url_is_set() {
    let tarball = format!("{ARTIFACTORY_REGISTRY}@acme/widget/-/@acme/widget-1.2.3.tgz");
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball,
        integrity: Some(integrity(SHA512)),
        revision: None,
        git_hosted: None,
        path: None,
    });
    let actual =
        resolution.to_lockfile_form("@acme/widget", "1.2.3", artifactory_form(true)).unwrap();
    assert_eq!(actual, resolution);
}

/// A registry declared to behave like registry.npmjs.org gets its leniency:
/// the percent-encoded scoped path is reconstructible there too. Undeclared,
/// the same URL must survive — the registry may serve only the encoded path.
/// See <https://github.com/pnpm/pnpm/issues/13534>.
#[test]
fn to_lockfile_form_drops_the_encoded_scoped_path_only_when_the_registry_is_declared_npm() {
    let registry = "https://npm.example.com/";
    let resolution = LockfileResolution::Tarball(TarballResolution {
        tarball: format!("{registry}@babel%2Fcore/-/core-7.0.0.tgz"),
        integrity: Some(integrity(SHA512)),
        revision: None,
        git_hosted: None,
        path: None,
    });

    let declared_npm = LockfileFormOptions {
        registry,
        server_type: Some(RegistryServerType::Npm),
        include_tarball_url: false,
    };
    assert_eq!(
        resolution.to_lockfile_form("@babel/core", "7.0.0", declared_npm).unwrap(),
        LockfileResolution::Registry(RegistryResolution {
            integrity: integrity(SHA512),
            revision: None
        }),
    );
    assert_eq!(
        resolution
            .to_lockfile_form("@babel/core", "7.0.0", undeclared_form(registry, false))
            .unwrap(),
        resolution,
    );
}
