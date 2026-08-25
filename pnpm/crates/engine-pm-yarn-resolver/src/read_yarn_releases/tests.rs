use super::{ReadYarnReleasesError, asset_variants, parse_releases};
use pnpm_lockfile::LockfileResolution;
use pretty_assertions::assert_eq;

/// Shaped like the `yarnpkg/zpm` release API's answer, down to the
/// target triples and `sha256:<hex>` digests it reports.
fn releases_body(extra_assets: &str) -> String {
    format!(
        r#"[
          {{
            "tag_name": "v6.0.0-rc.19",
            "assets": [
              {{
                "name": "yarn-aarch64-apple-darwin.zip",
                "browser_download_url": "https://github.com/yarnpkg/zpm/releases/download/v6.0.0-rc.19/yarn-aarch64-apple-darwin.zip",
                "digest": "sha256:3485e3aec467ec56d26e3b3fed027abe82138fafdb7d8ae4f068ec32f6fe40ad"
              }},
              {{
                "name": "yarn-x86_64-unknown-linux-musl.zip",
                "browser_download_url": "https://github.com/yarnpkg/zpm/releases/download/v6.0.0-rc.19/yarn-x86_64-unknown-linux-musl.zip",
                "digest": "sha256:d2fee036a3a79d224e73c1dc5af68742034ca40a2c2daf954ae7a6a7fbcde593"
              }}{extra_assets}
            ]
          }},
          {{
            "tag_name": "v6.0.0-rc.18",
            "assets": []
          }}
        ]"#,
    )
}

#[test]
fn releases_are_read_with_the_v_prefix_stripped() {
    let releases = parse_releases(&releases_body("")).expect("parse the release list");
    assert_eq!(
        releases.iter().map(|release| release.version.as_str()).collect::<Vec<_>>(),
        ["6.0.0-rc.19", "6.0.0-rc.18"],
    );
}

#[test]
fn archives_become_platform_variants_with_sri_integrities() {
    let releases = parse_releases(&releases_body("")).expect("parse the release list");
    let variants = asset_variants(&releases[0]).expect("decode the archives");

    let described: Vec<(String, String, Option<String>, String)> = variants
        .iter()
        .map(|variant| {
            let target = &variant.targets[0];
            let integrity = match &variant.resolution {
                LockfileResolution::Binary(binary) => binary.integrity.to_string(),
                other => panic!("expected a binary resolution, got {other:?}"),
            };
            (target.os.clone(), target.cpu.clone(), target.libc.clone(), integrity)
        })
        .collect();
    assert_eq!(
        described,
        [
            (
                "darwin".to_string(),
                "arm64".to_string(),
                None,
                "sha256-NIXjrsRn7FbSbjs/7QJ6voITj6/bfYrk8GjsMvb+QK0=".to_string(),
            ),
            (
                "linux".to_string(),
                "x64".to_string(),
                None,
                "sha256-0v7gNqOnnSJOc8HcWvaHQgNMpAosLa+VSuemp/vN5ZM=".to_string(),
            ),
        ],
    );
}

/// zpm's Linux archive is a static musl build, so it must stay usable on
/// a glibc host — until a release actually ships both, at which point the
/// two have to be told apart.
#[test]
fn the_musl_constraint_is_recorded_only_when_a_glibc_build_exists() {
    let glibc_asset = r#",
              {
                "name": "yarn-x86_64-unknown-linux-gnu.zip",
                "browser_download_url": "https://github.com/yarnpkg/zpm/releases/download/v6.0.0-rc.19/yarn-x86_64-unknown-linux-gnu.zip",
                "digest": "sha256:9618233c0b659ca716ab1c591a00aacc0facc39e7ac7119042ce6b4fd3840bce"
              }"#;
    let releases = parse_releases(&releases_body(glibc_asset)).expect("parse the release list");
    let variants = asset_variants(&releases[0]).expect("decode the archives");

    let musl: Vec<Option<String>> = variants
        .iter()
        .filter(|variant| variant.targets[0].os == "linux")
        .map(|variant| variant.targets[0].libc.clone())
        .collect();
    assert_eq!(musl, [None, Some("musl".to_string())]);
}

/// A glibc build pnpm cannot verify is not a build to choose between, so
/// it must not push the musl archive out of reach of a glibc host.
#[test]
fn an_unverifiable_glibc_build_leaves_the_musl_archive_unconstrained() {
    let unsigned_glibc = r#",
              {
                "name": "yarn-x86_64-unknown-linux-gnu.zip",
                "browser_download_url": "https://github.com/yarnpkg/zpm/releases/download/v6.0.0-rc.19/yarn-x86_64-unknown-linux-gnu.zip",
                "digest": "md5:0123456789abcdef0123456789abcdef"
              }"#;
    let releases = parse_releases(&releases_body(unsigned_glibc)).expect("parse the release list");
    let variants = asset_variants(&releases[0]).expect("decode the archives");

    let linux: Vec<Option<String>> = variants
        .iter()
        .filter(|variant| variant.targets[0].os == "linux")
        .map(|variant| variant.targets[0].libc.clone())
        .collect();
    assert_eq!(linux, [None]);
}

#[test]
fn an_asset_without_a_usable_digest_is_skipped() {
    let unsigned = r#",
              {
                "name": "yarn-i686-unknown-linux-musl.zip",
                "browser_download_url": "https://github.com/yarnpkg/zpm/releases/download/v6.0.0-rc.19/yarn-i686-unknown-linux-musl.zip",
                "digest": "md5:0123456789abcdef0123456789abcdef"
              }"#;
    let releases = parse_releases(&releases_body(unsigned)).expect("parse the release list");
    let variants = asset_variants(&releases[0]).expect("decode the archives");
    assert!(
        variants.iter().all(|variant| variant.targets[0].cpu != "ia32"),
        "an archive pnpm cannot verify must not be installable",
    );
}

#[test]
fn a_release_without_archives_is_an_error() {
    let releases = parse_releases(&releases_body("")).expect("parse the release list");
    let error = asset_variants(&releases[1]).expect_err("a release with no assets cannot resolve");
    assert!(matches!(error, ReadYarnReleasesError::NoUsableAssets { .. }), "{error:?}");
}
