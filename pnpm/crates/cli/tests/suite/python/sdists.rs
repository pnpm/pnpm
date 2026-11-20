use super::{
    TINY_BACKEND,
    approve,
    assert_failure_contains,
    project,
    python,
    sdist,
    sdist_zip,
    serve,
    serve_archives,
    serve_backends,
    serve_wheels,
    wheel,
};
use crate::_utils::pacquet_in;
use assert_cmd::prelude::*;
use std::fs;

/// The `getsentry/sentry` and `marimo-team/marimo` case of item 5 of
/// pnpm/pnpm#14945: a requirement whose distribution has never published
/// a wheel.
#[tokio::test]
async fn a_release_with_only_a_source_distribution_is_built_and_replayed_offline() {
    for zip in [false, true] {
        eprintln!("zip: {zip}");
        let root = tempfile::tempdir().unwrap();
        let mut server = mockito::Server::new_async().await;
        let archive = if zip {
            ("alpha-1.0.zip".to_string(), sdist_zip("alpha", "1.0", &["helper>=1"]))
        } else {
            ("alpha-1.0.tar.gz".to_string(), sdist("alpha", "1.0", &["helper>=1"]))
        };
        let filename = archive.0.clone();
        let _alpha = serve_archives(&mut server, "alpha", &[archive]).await;
        let _helper =
            serve(&mut server, "helper", &[("1.0", wheel("helper", "1.0", "", &[]))]).await;
        let _backends = serve_backends(&mut server).await;
        project(root.path(), &server.url(), &["alpha>=1"]);
        approve(root.path(), "alpha");

        pacquet_in(root.path())
            .arg("install")
            .assert()
            .success();

        python(root.path())
            .args(["-c", "import alpha, helper"])
            .assert()
            .success();
        let lock = fs::read_to_string(root.path().join("pylock.toml")).unwrap();
        eprintln!("pylock.toml:\n{lock}");
        let pinned = format!(
            "[[packages]]\nname = \"alpha\"\nversion = \"1.0\"\n\n[packages.sdist]\nname = \"{filename}\"",
        );
        assert!(lock.contains(&pinned), "the lockfile pins the archive and no wheel of alpha");

        pnpm_fs::remove_symlink_dir(&root.path().join(".venv")).unwrap();
        pacquet_in(root.path())
            .args(["install", "--offline", "--frozen-lockfile"])
            .assert()
            .success();
        python(root.path())
            .args(["-c", "import alpha, helper"])
            .assert()
            .success();
        assert_eq!(fs::read_to_string(root.path().join("pylock.toml")).unwrap(), lock);
    }
}

/// A legacy source distribution declares its requirements only in the
/// wheel its backend builds, so the manifest beside it — here, none at
/// all — is not a second answer pnpm holds the wheel to.
#[tokio::test]
async fn a_source_distribution_without_a_manifest_is_read_from_the_wheel_it_builds() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let mut archive =
        tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
    for (path, contents) in
        [("setup.py", "# a legacy Python project\n"), ("alpha/__init__.py", "VERSION = '1.0'\n")]
    {
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive
            .append_data(&mut header, format!("alpha-1.0/{path}"), contents.as_bytes())
            .unwrap();
    }
    let archive = archive
        .into_inner()
        .unwrap()
        .finish()
        .unwrap();
    let _alpha =
        serve_archives(&mut server, "alpha", &[("alpha-1.0.tar.gz".to_string(), archive)]).await;
    let _helper = serve(&mut server, "helper", &[("1.0", wheel("helper", "1.0", "", &[]))]).await;
    let backend = TINY_BACKEND.replace(
        r#"manifest = tomllib.load(open("pyproject.toml", "rb"))"#,
        "manifest = {'project': {'name': 'alpha', 'version': '1.0', 'dependencies': \
         ['helper>=1']}}",
    );
    let legacy = format!("{backend}\nimport sys\n__legacy__ = sys.modules[__name__]\n");
    let _setuptools = serve(
        &mut server,
        "setuptools",
        &[("80.0", wheel("setuptools", "80.0", "", &[("setuptools/build_meta.py", &legacy)]))],
    )
    .await;
    let _wheel = serve(&mut server, "wheel", &[("80.0", wheel("wheel", "80.0", "", &[]))]).await;
    project(root.path(), &server.url(), &["alpha>=1"]);
    approve(root.path(), "alpha");

    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();

    python(root.path())
        .args(["-c", "import alpha, helper"])
        .assert()
        .success();
}

#[tokio::test]
async fn a_source_distribution_is_not_built_without_its_own_approval() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve_archives(
        &mut server,
        "alpha",
        &[("alpha-1.0.tar.gz".to_string(), sdist("alpha", "1.0", &[]))],
    )
    .await;
    let _backends = serve_backends(&mut server).await;
    project(root.path(), &server.url(), &["alpha>=1"]);

    assert_failure_contains(
        pacquet_in(root.path()).arg("install"),
        "requires approval of pkg:pypi/alpha under allowBuilds",
    );
    assert!(!root.path().join("pylock.toml").exists(), "a refused build published a lockfile");
}

#[tokio::test]
async fn a_wheel_is_taken_over_the_source_distribution_of_its_own_release() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve_archives(
        &mut server,
        "alpha",
        &[
            ("alpha-1.0-py3-none-any.whl".to_string(), wheel("alpha", "1.0", "", &[])),
            ("alpha-1.0.tar.gz".to_string(), sdist("alpha", "1.0", &[])),
        ],
    )
    .await;
    let unbuilt = server
        .mock("GET", "/files/alpha-1.0.tar.gz")
        .expect(0)
        .create_async()
        .await;
    project(root.path(), &server.url(), &["alpha>=1"]);

    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();

    let lock = fs::read_to_string(root.path().join("pylock.toml")).unwrap();
    eprintln!("pylock.toml:\n{lock}");
    assert!(!lock.contains("sdist"), "the lockfile pins the source distribution");
    unbuilt.assert_async().await;
}

/// Item 5 of pnpm/pnpm#14945 also asks that the empty version set a
/// failed resolution prints say which of its causes it was.
#[tokio::test]
async fn a_resolution_failure_says_what_the_index_offered() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let missing = server
        .mock("GET", "/simple/alpha/")
        .with_status(404)
        .expect_at_least(1)
        .create_async()
        .await;
    project(root.path(), &server.url(), &["alpha>=1"]);

    assert_failure_contains(
        pacquet_in(root.path()).arg("install"),
        "No index pnpm reads publishes a distribution named alpha",
    );
    missing.assert_async().await;

    let mut server = mockito::Server::new_async().await;
    let _beta = serve_wheels(&mut server, "beta", "1.0", &["py3-none-sunos5"]).await;
    project(root.path(), &server.url(), &["beta>=1"]);

    assert_failure_contains(
        pacquet_in(root.path()).arg("install"),
        "beta publishes 1 releases (1.0), none of which publishes a wheel this interpreter \
         installs or a source distribution pnpm can build",
    );

    let mut server = mockito::Server::new_async().await;
    let _gamma = serve(&mut server, "gamma", &[("1.0", wheel("gamma", "1.0", "", &[]))]).await;
    project(root.path(), &server.url(), &["gamma>=2"]);

    assert_failure_contains(
        pacquet_in(root.path()).arg("install"),
        "gamma is offered at 1.0, and this project's requirements select none of them",
    );
}

/// A release with a wheel for one environment and only a source
/// distribution for another is one PEP 751 package carrying both. The
/// environment that has a wheel takes it; the others build the archive.
#[tokio::test]
async fn one_release_pins_the_wheel_an_environment_takes_beside_the_archive_the_others_build() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let (_, tag) = super::running_platform();
    let _alpha = serve_archives(
        &mut server,
        "alpha",
        &[
            (
                format!("alpha-1.0-{tag}.whl"),
                super::wheel_with_tags("alpha", "1.0", "", &[], &format!("Tag: {tag}\n")),
            ),
            ("alpha-1.0.tar.gz".to_string(), sdist("alpha", "1.0", &[])),
        ],
    )
    .await;
    project(root.path(), &server.url(), &["alpha>=1"]);
    super::add_supported_architectures(
        root.path(),
        &super::declared_platforms()
            .iter()
            .map(|(platform, _)| *platform)
            .collect::<Vec<_>>(),
    );

    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();

    let lock = fs::read_to_string(root.path().join("pylock.toml")).unwrap();
    eprintln!("pylock.toml:\n{lock}");
    assert!(lock.contains(&format!(r#"name = "alpha-1.0-{tag}.whl""#)), "no wheel was pinned");
    assert!(lock.contains("[packages.sdist]"), "no source distribution was pinned");
    python(root.path())
        .args(["-c", "import alpha"])
        .assert()
        .success();
}

/// GNU tar writes its entries with a `./` prefix, and the shared
/// extractor spends its one stripped component on that prefix rather
/// than on the release directory. The source tree has to be rooted at
/// the manifest either way, or the backend runs somewhere that has none.
#[tokio::test]
async fn a_source_distribution_whose_entries_are_dot_prefixed_builds() {
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _alpha = serve_archives(
        &mut server,
        "alpha",
        &[(
            "alpha-1.0.tar.gz".to_string(),
            super::sdist_dot_prefixed("alpha", "1.0", &["helper>=1"]),
        )],
    )
    .await;
    let _helper = serve(&mut server, "helper", &[("1.0", wheel("helper", "1.0", "", &[]))]).await;
    let _backends = serve_backends(&mut server).await;
    project(root.path(), &server.url(), &["alpha>=1"]);
    approve(root.path(), "alpha");

    pacquet_in(root.path())
        .arg("install")
        .assert()
        .success();

    python(root.path())
        .args(["-c", "import alpha, helper"])
        .assert()
        .success();
}
