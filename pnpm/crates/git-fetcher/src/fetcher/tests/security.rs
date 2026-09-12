use super::{
    GitFetcherError, GitManifestQuery, PreparePackageError, fs, make_bare_repo, read_git_manifest,
    tempdir,
};

/// A `path` that climbs out of the checkout must not reach
/// `safe_read_package_json_from_dir`, which would read an arbitrary
/// `package.json` off the host and stamp its name onto this dep.
#[tokio::test(flavor = "multi_thread")]
async fn read_git_manifest_rejects_a_sub_directory_escape() {
    let tmp = tempdir().unwrap();
    let (bare, commit) = make_bare_repo(tmp.path());
    let repo = format!("file://{}", bare.to_string_lossy());
    // A real `package.json` sitting outside the checkout, of the shape
    // an escape would be aiming for.
    fs::write(tmp.path().join("package.json"), r#"{"name":"outside","version":"9.9.9"}"#).unwrap();

    for escape in ["/../..", "../..", "/../"] {
        let err = read_git_manifest(GitManifestQuery {
            source_cache: &crate::GitSourceCache::default(),
            repo: &repo,
            commit: &commit,
            path: Some(escape),
            git_shallow_hosts: &[],
            git_bin: None,
        })
        .await
        .expect_err("a path climbing out of the checkout must be rejected");
        assert!(
            matches!(err, GitFetcherError::Prepare(PreparePackageError::InvalidPath { .. })),
            "{escape:?} produced {err:?}",
        );
    }
}
