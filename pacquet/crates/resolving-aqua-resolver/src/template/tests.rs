use super::{ExpandedAsset, expand_assets, expand_checksum_asset_name};
use crate::registry::{AquaFile, AquaOverride, AquaVersionOverride, Replacements, ResolvedSpec};

fn replacements(pairs: &[(&str, &str)]) -> Replacements {
    pairs.iter().map(|(key, value)| ((*key).to_string(), (*value).to_string())).collect()
}

fn find<'a>(assets: &'a [ExpandedAsset], os: &str, cpu: &str) -> &'a ExpandedAsset {
    assets
        .iter()
        .find(|asset| asset.target.os == os && asset.target.cpu == cpu)
        .unwrap_or_else(|| panic!("no asset for {os}/{cpu}"))
}

#[test]
fn expands_ripgrep_style_asset_templates() {
    let over = AquaVersionOverride {
        version_constraint: Some("true".to_string()),
        asset: Some("ripgrep-{{.Version}}-{{.Arch}}-{{.OS}}.{{.Format}}".to_string()),
        format: Some("tar.gz".to_string()),
        files: Some(vec![AquaFile {
            name: "rg".to_string(),
            src: Some("ripgrep-{{.Version}}-{{.Arch}}-{{.OS}}/rg".to_string()),
        }]),
        replacements: Some(replacements(&[
            ("amd64", "x86_64"),
            ("arm64", "aarch64"),
            ("darwin", "apple-darwin"),
            ("windows", "pc-windows-msvc"),
        ])),
        overrides: Some(vec![
            AquaOverride {
                goos: Some("linux".to_string()),
                goarch: Some("amd64".to_string()),
                replacements: Some(replacements(&[("linux", "unknown-linux-musl")])),
                ..AquaOverride::default()
            },
            AquaOverride {
                goos: Some("linux".to_string()),
                goarch: Some("arm64".to_string()),
                replacements: Some(replacements(&[("linux", "unknown-linux-gnu")])),
                ..AquaOverride::default()
            },
            AquaOverride {
                goos: Some("windows".to_string()),
                format: Some("zip".to_string()),
                ..AquaOverride::default()
            },
        ]),
        ..AquaVersionOverride::default()
    };
    let assets = expand_assets("BurntSushi", "ripgrep", "14.1.1", &ResolvedSpec::from(&over));

    let darwin_arm = find(&assets, "darwin", "arm64");
    assert_eq!(
        darwin_arm.url,
        "https://github.com/BurntSushi/ripgrep/releases/download/14.1.1/ripgrep-14.1.1-aarch64-apple-darwin.tar.gz",
    );
    assert_eq!(darwin_arm.format, "tar.gz");

    let linux_amd = find(&assets, "linux", "x64");
    assert_eq!(
        linux_amd.url,
        "https://github.com/BurntSushi/ripgrep/releases/download/14.1.1/ripgrep-14.1.1-x86_64-unknown-linux-musl.tar.gz",
    );

    let win_amd = find(&assets, "win32", "x64");
    assert_eq!(
        win_amd.url,
        "https://github.com/BurntSushi/ripgrep/releases/download/14.1.1/ripgrep-14.1.1-x86_64-pc-windows-msvc.zip",
    );
    assert_eq!(win_amd.format, "zip");
}

#[test]
fn expands_fzf_style_asset_templates_with_trim_v() {
    let over = AquaVersionOverride {
        version_constraint: Some("true".to_string()),
        asset: Some("fzf-{{trimV .Version}}-{{.OS}}_{{.Arch}}.{{.Format}}".to_string()),
        format: Some("tar.gz".to_string()),
        overrides: Some(vec![AquaOverride {
            goos: Some("windows".to_string()),
            format: Some("zip".to_string()),
            ..AquaOverride::default()
        }]),
        ..AquaVersionOverride::default()
    };
    let assets = expand_assets("junegunn", "fzf", "v0.57.0", &ResolvedSpec::from(&over));

    assert_eq!(
        find(&assets, "darwin", "arm64").url,
        "https://github.com/junegunn/fzf/releases/download/v0.57.0/fzf-0.57.0-darwin_arm64.tar.gz",
    );
    assert_eq!(
        find(&assets, "win32", "x64").url,
        "https://github.com/junegunn/fzf/releases/download/v0.57.0/fzf-0.57.0-windows_amd64.zip",
    );
}

#[test]
fn filters_platforms_based_on_supported_envs() {
    let over = AquaVersionOverride {
        version_constraint: Some("true".to_string()),
        asset: Some("tool-{{.OS}}-{{.Arch}}.tar.gz".to_string()),
        format: Some("tar.gz".to_string()),
        supported_envs: Some(vec!["linux/amd64".to_string(), "darwin".to_string()]),
        ..AquaVersionOverride::default()
    };
    let assets = expand_assets("test", "tool", "v1.0.0", &ResolvedSpec::from(&over));

    let oses: Vec<String> =
        assets.iter().map(|asset| format!("{}/{}", asset.target.os, asset.target.cpu)).collect();
    assert!(oses.contains(&"linux/x64".to_string()));
    assert!(oses.contains(&"darwin/arm64".to_string()));
    assert!(oses.contains(&"darwin/x64".to_string()));
    assert!(!oses.contains(&"win32/x64".to_string()));
    assert!(!oses.contains(&"linux/arm64".to_string()));
}

#[test]
fn returns_no_assets_when_the_spec_has_no_asset_template() {
    let over = AquaVersionOverride {
        version_constraint: Some("true".to_string()),
        ..AquaVersionOverride::default()
    };
    assert!(expand_assets("test", "tool", "v1.0.0", &ResolvedSpec::from(&over)).is_empty());
}

#[test]
fn expands_checksum_asset_template_with_asset() {
    let result = expand_checksum_asset_name(
        "{{.Asset}}.sha256",
        "ripgrep-14.1.1-x86_64-apple-darwin.tar.gz",
        "14.1.1",
    );
    assert_eq!(result, "ripgrep-14.1.1-x86_64-apple-darwin.tar.gz.sha256");
}

#[test]
fn expands_checksum_asset_template_with_trim_v() {
    let result = expand_checksum_asset_name(
        "fzf_{{trimV .Version}}_checksums.txt",
        "fzf-0.57.0-darwin_arm64.tar.gz",
        "v0.57.0",
    );
    assert_eq!(result, "fzf_0.57.0_checksums.txt");
}
