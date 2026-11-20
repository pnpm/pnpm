use super::SupportedArchitecturesArgs;
use pnpm_package_is_installable::{
    ArchitectureAxes,
    SupportedArchitectures,
};
use pretty_assertions::assert_eq;

#[test]
fn empty_cli_passes_existing_through() {
    let cli = SupportedArchitecturesArgs::default();
    let existing = Some(SupportedArchitectures::Axes(ArchitectureAxes {
        os: Some(vec!["darwin".to_string()]),
        cpu: None,
        libc: None,
    }));
    assert_eq!(cli.apply_to(existing.clone()), existing);
}

#[test]
fn cli_cpu_replaces_config_cpu_only() {
    let cli = SupportedArchitecturesArgs { cpu: vec!["x64".to_string()], os: vec![], libc: vec![] };
    let existing = Some(SupportedArchitectures::Axes(ArchitectureAxes {
        os: Some(vec!["darwin".to_string()]),
        cpu: Some(vec!["arm64".to_string()]),
        libc: None,
    }));
    assert_eq!(
        cli.apply_to(existing),
        Some(SupportedArchitectures::Axes(ArchitectureAxes {
            os: Some(vec!["darwin".to_string()]),
            cpu: Some(vec!["x64".to_string()]),
            libc: None,
        })),
    );
}

#[test]
fn cli_without_existing_creates_supported_architectures() {
    let cli = SupportedArchitecturesArgs {
        cpu: vec!["x64".to_string()],
        os: vec!["linux".to_string()],
        libc: vec!["glibc".to_string()],
    };
    assert_eq!(
        cli.apply_to(None),
        Some(SupportedArchitectures::Axes(ArchitectureAxes {
            os: Some(vec!["linux".to_string()]),
            cpu: Some(vec!["x64".to_string()]),
            libc: Some(vec!["glibc".to_string()]),
        })),
    );
}

/// The flags name axes, so they replace a configuration that named its
/// platforms rather than narrowing it.
#[test]
fn cli_axes_replace_a_configured_platform_list() {
    let cli = SupportedArchitecturesArgs { cpu: vec!["x64".to_string()], os: vec![], libc: vec![] };
    let existing = Some(SupportedArchitectures::Platforms(vec![
        "linux-arm64".parse().unwrap(),
        "darwin-arm64".parse().unwrap(),
    ]));
    assert_eq!(
        cli.apply_to(existing),
        Some(SupportedArchitectures::Axes(ArchitectureAxes {
            os: None,
            cpu: Some(vec!["x64".to_string()]),
            libc: None,
        })),
    );
}
