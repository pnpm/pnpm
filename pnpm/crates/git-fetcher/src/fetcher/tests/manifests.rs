use super::{
    AllowBuildRef, GitFetcher, GitFetcherError, GitManifestQuery, ScriptsPrependNodePath,
    SilentReporter, StoreDir, Value, deny_all_builds, make_bare_repo,
    make_bare_repo_with_prepare_script, make_bare_repo_with_sub_package,
    make_bare_repo_without_manifest, read_git_manifest, tempdir,
};

#[tokio::test(flavor = "multi_thread")]
async fn fetcher_handles_repo_without_package_json() {
    let tmp = tempdir().unwrap();
    let (bare, commit) = make_bare_repo_without_manifest(tmp.path());
    let store_root = tempdir().unwrap();
    let store_dir = StoreDir::from(store_root.path().to_path_buf());

    let repo_url = format!("file://{}", bare.display());
    let received = GitFetcher {
        source_cache: &crate::GitSourceCache::default(),
        repo: &repo_url,
        commit: &commit,
        path: None,
        git_shallow_hosts: &[],
        allow_build: deny_all_builds(),
        ignore_scripts: false,
        unsafe_perm: true,
        user_agent: None,
        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        script_shell: None,
        node_execpath: None,
        npm_execpath: None,
        pnpm_execpath: None,
        store_dir: &store_dir,
        package_id: "anon@0.0.0",
        package_name: "pkg",
        requester: "/test",
        store_index_writer: None,
        files_index_file: "anon@0.0.0\tbuilt",
        git_bin: None,
    }
    .run::<SilentReporter>()
    .await
    .unwrap();

    assert!(!received.built, "no manifest → not built");
    assert!(received.cas_paths.contains_key("README.md"));
    assert!(received.cas_paths.contains_key("index.js"));
}

#[tokio::test(flavor = "multi_thread")]
async fn fetcher_rejects_untrusted_manifest_identity() {
    let tmp = tempdir().unwrap();
    let (bare, commit) = make_bare_repo_with_prepare_script(
        tmp.path(),
        r#"node -e "require('fs').writeFileSync('BUILD_RAN.marker', 'ok')""#,
    );
    let store_root = tempdir().unwrap();
    let store_dir = StoreDir::from(store_root.path().to_path_buf());
    let repo_url = format!("file://{}", bare.display());
    let allow_registry_artifacts_only: AllowBuildRef<'_> = &|dep_path| !dep_path.contains("://");

    let err = GitFetcher {
        source_cache: &crate::GitSourceCache::default(),
        repo: &repo_url,
        commit: &commit,
        path: None,
        git_shallow_hosts: &[],
        allow_build: allow_registry_artifacts_only,
        ignore_scripts: false,
        unsafe_perm: true,
        user_agent: None,
        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        script_shell: None,
        node_execpath: None,
        npm_execpath: None,
        pnpm_execpath: None,
        store_dir: &store_dir,
        package_id: "git+file:///tmp/repo.git#abc123",
        package_name: "pkg",
        requester: "/test",
        store_index_writer: None,
        files_index_file: "git+file:///tmp/repo.git#abc123\tbuilt",
        git_bin: None,
    }
    .run::<SilentReporter>()
    .await
    .unwrap_err();

    match err {
        GitFetcherError::Prepare(crate::error::PreparePackageError::NotAllowed {
            name,
            version,
            ..
        }) => {
            assert_eq!(name, "x");
            assert_eq!(version, "1.0.0");
        }
        other => panic!("expected NotAllowed, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn fetcher_allows_untrusted_manifest_identity_by_dep_path() {
    let tmp = tempdir().unwrap();
    let (bare, commit) = make_bare_repo_with_prepare_script(
        tmp.path(),
        r#"node -e "require('fs').writeFileSync('BUILD_RAN.marker', 'ok')""#,
    );
    let store_root = tempdir().unwrap();
    let store_dir = StoreDir::from(store_root.path().to_path_buf());
    let repo_url = format!("file://{}", bare.display());
    let package_id = "git+file:///tmp/repo.git#abc123";
    let allow_dep_path: AllowBuildRef<'_> =
        &|dep_path| dep_path == "x@git+file:///tmp/repo.git#abc123";

    let received = GitFetcher {
        source_cache: &crate::GitSourceCache::default(),
        repo: &repo_url,
        commit: &commit,
        path: None,
        git_shallow_hosts: &[],
        allow_build: allow_dep_path,
        ignore_scripts: false,
        unsafe_perm: true,
        user_agent: None,
        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        script_shell: None,
        node_execpath: None,
        npm_execpath: None,
        pnpm_execpath: None,
        store_dir: &store_dir,
        package_id,
        package_name: "pkg",
        requester: "/test",
        store_index_writer: None,
        files_index_file: "git+file:///tmp/repo.git#abc123\tbuilt",
        git_bin: None,
    }
    .run::<SilentReporter>()
    .await
    .unwrap();

    assert!(received.built);
    assert!(received.cas_paths.contains_key("BUILD_RAN.marker"));
}

/// A `Git` resolution's package name lives only in the working tree —
/// the specifier names a repo, not a package.
#[tokio::test(flavor = "multi_thread")]
async fn read_git_manifest_reads_the_name_from_the_checkout() {
    let tmp = tempdir().unwrap();
    let (bare, commit) = make_bare_repo(tmp.path());
    let repo = format!("file://{}", bare.to_string_lossy());

    let manifest = read_git_manifest(GitManifestQuery {
        source_cache: &crate::GitSourceCache::default(),
        repo: &repo,
        commit: &commit,
        path: None,
        git_shallow_hosts: &[],
        git_bin: None,
    })
    .await
    .expect("checkout should be readable");

    let manifest = dbg!(manifest).expect("repo root has a package.json");
    assert_eq!(manifest.get("name").and_then(Value::as_str), Some("pkg"));
    assert_eq!(manifest.get("version").and_then(Value::as_str), Some("1.0.0"));
}

/// `#path:/packages/foo` keeps its leading slash, which is rooted at
/// the repo rather than the filesystem.
#[tokio::test(flavor = "multi_thread")]
async fn read_git_manifest_reads_a_repo_rooted_sub_directory() {
    let tmp = tempdir().unwrap();
    let (bare, commit) = make_bare_repo_with_sub_package(tmp.path());
    let repo = format!("file://{}", bare.to_string_lossy());

    let manifest = read_git_manifest(GitManifestQuery {
        source_cache: &crate::GitSourceCache::default(),
        repo: &repo,
        commit: &commit,
        path: Some("/packages/foo"),
        git_shallow_hosts: &[],
        git_bin: None,
    })
    .await
    .expect("checkout should be readable");

    let manifest = dbg!(manifest).expect("sub-directory has a package.json");
    assert_eq!(manifest.get("name").and_then(Value::as_str), Some("@scope/foo"));
}

/// Degrades to `None` rather than failing the resolve, matching the
/// archive path's best-effort contract.
#[tokio::test(flavor = "multi_thread")]
async fn read_git_manifest_returns_none_for_a_directory_without_a_manifest() {
    let tmp = tempdir().unwrap();
    let (bare, commit) = make_bare_repo_with_sub_package(tmp.path());
    let repo = format!("file://{}", bare.to_string_lossy());

    let manifest = read_git_manifest(GitManifestQuery {
        source_cache: &crate::GitSourceCache::default(),
        repo: &repo,
        commit: &commit,
        path: Some("/packages/no-manifest"),
        git_shallow_hosts: &[],
        git_bin: None,
    })
    .await
    .expect("a manifest-less directory is not a failure");

    assert_eq!(dbg!(manifest), None);
}

/// The commit guard protects the resolve-time checkout too: a value
/// starting with `-` would otherwise reach `git checkout` as a flag.
#[tokio::test(flavor = "multi_thread")]
async fn read_git_manifest_rejects_a_non_sha_commit() {
    let tmp = tempdir().unwrap();
    let (bare, _) = make_bare_repo(tmp.path());
    let repo = format!("file://{}", bare.to_string_lossy());

    let err = read_git_manifest(GitManifestQuery {
        source_cache: &crate::GitSourceCache::default(),
        repo: &repo,
        commit: "--upload-pack=touch /tmp/pwned",
        path: None,
        git_shallow_hosts: &[],
        git_bin: None,
    })
    .await
    .expect_err("a non-SHA commit must be rejected before it reaches git");

    assert!(
        matches!(&err, GitFetcherError::SharedSource(source) if matches!(source.as_ref(), GitFetcherError::InvalidCommit { .. })),
        "{err:?}",
    );
}

/// A repo beginning with `-` is read by git as an option, not a URL:
/// `--upload-pack=<cmd>` runs `<cmd>` on a local or SSH transport. Both
/// passes reach `checkout_commit`, so both are guarded.
#[tokio::test(flavor = "multi_thread")]
async fn read_git_manifest_rejects_an_option_shaped_repo() {
    let tmp = tempdir().unwrap();
    let marker = tmp.path().join("pwned.txt");
    let payload = format!("--upload-pack=touch {}", marker.to_string_lossy());

    let err = read_git_manifest(GitManifestQuery {
        source_cache: &crate::GitSourceCache::default(),
        repo: &payload,
        commit: "0123456789abcdef0123456789abcdef01234567",
        path: None,
        git_shallow_hosts: &[],
        git_bin: None,
    })
    .await
    .expect_err("an option-shaped repo must be rejected before it reaches git");

    assert!(
        matches!(&err, GitFetcherError::SharedSource(source) if matches!(source.as_ref(), GitFetcherError::InvalidRepo { .. })),
        "{err:?}",
    );
    assert!(!marker.exists(), "the payload must never have run");
}
