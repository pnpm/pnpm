use super::{TempDir, finder};
use pnpm_hooks::PnpmfileHooks as _;

#[tokio::test]
async fn calculate_pnpmfile_checksum_hashes_normalized_contents_when_hooks_exported() {
    let tmp = TempDir::new().expect("temp dir");
    let pnpmfile_path = tmp.path().join(".pnpmfile.cjs");
    let src = "module.exports = { hooks: { readPackage: (pkg) => pkg } }\n";
    std::fs::write(&pnpmfile_path, src).expect("write pnpmfile");

    let hooks = pnpm_hooks::node_runtime::NodeJsHooks::new(pnpmfile_path);
    let checksum = hooks.calculate_pnpmfile_checksum().await;

    assert_eq!(checksum, Some(pnpm_crypto_hash::create_hash(src)));
}

#[tokio::test]
async fn calculate_pnpmfile_checksum_is_none_when_no_hooks_exported() {
    let tmp = TempDir::new().expect("temp dir");
    let pnpmfile_path = tmp.path().join(".pnpmfile.cjs");
    std::fs::write(&pnpmfile_path, "module.exports = {}\n").expect("write pnpmfile");

    let hooks = pnpm_hooks::node_runtime::NodeJsHooks::new(pnpmfile_path);

    assert_eq!(hooks.calculate_pnpmfile_checksum().await, None);
}

/// No pnpmfile means no checksum, so a lockfile that records one is
/// drift — the removal itself is what the gate must see.
#[tokio::test]
async fn current_pnpmfile_checksum_is_none_without_a_pnpmfile() {
    assert_eq!(pnpm_hooks::current_pnpmfile_checksum(None, Some("sha256-abc")).await, None);
}

#[tokio::test]
async fn current_pnpmfile_checksum_hashes_the_pnpmfile_that_exports_hooks() {
    let tmp = TempDir::new().expect("temp dir");
    let src = "module.exports = { hooks: { readPackage: (pkg) => pkg } }\n";
    std::fs::write(tmp.path().join(".pnpmfile.cjs"), src).expect("write pnpmfile");

    let hooks = finder::load_pnpmfile(tmp.path());
    assert_eq!(
        pnpm_hooks::current_pnpmfile_checksum(hooks.as_ref(), None).await,
        Some(pnpm_crypto_hash::create_hash(src)),
    );
}

/// pnpm records no checksum for a pnpmfile that exports no `hooks`
/// object, so neither may pacquet — otherwise every install in a
/// project with a resolvers-only pnpmfile would report drift.
#[tokio::test]
async fn current_pnpmfile_checksum_is_none_when_the_pnpmfile_exports_no_hooks() {
    let tmp = TempDir::new().expect("temp dir");
    std::fs::write(tmp.path().join(".pnpmfile.cjs"), "module.exports = {}\n")
        .expect("write pnpmfile");

    let hooks = finder::load_pnpmfile(tmp.path());
    assert_eq!(pnpm_hooks::current_pnpmfile_checksum(hooks.as_ref(), None).await, None);
}

/// A lockfile that already records a checksum settles the comparison
/// by hash alone. Pinned with a pnpmfile that exports no hooks —
/// evaluating it would answer `None` and call an unchanged project
/// drifted.
#[tokio::test]
async fn current_pnpmfile_checksum_trusts_the_hash_when_the_lockfile_records_one() {
    let tmp = TempDir::new().expect("temp dir");
    let src = "module.exports = {}\n";
    std::fs::write(tmp.path().join(".pnpmfile.cjs"), src).expect("write pnpmfile");

    let hooks = finder::load_pnpmfile(tmp.path());
    let recorded = pnpm_crypto_hash::create_hash(src);
    assert_eq!(
        pnpm_hooks::current_pnpmfile_checksum(hooks.as_ref(), Some(&recorded)).await,
        Some(recorded),
    );
}
