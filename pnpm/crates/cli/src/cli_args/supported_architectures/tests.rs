use super::SupportedArchitecturesArgs;
use pacquet_package_is_installable::SupportedArchitectures;
use pretty_assertions::assert_eq;

#[test]
fn empty_cli_passes_existing_through() {
    let cli = SupportedArchitecturesArgs::default();
    let existing = Some(SupportedArchitectures {
        os: Some(vec!["darwin".to_string()]),
        cpu: None,
        libc: None,
    });
    assert_eq!(cli.apply_to(existing.clone()), existing);
}

#[test]
fn cli_cpu_replaces_config_cpu_only() {
    let cli = SupportedArchitecturesArgs { cpu: vec!["x64".to_string()], os: vec![], libc: vec![] };
    let existing = Some(SupportedArchitectures {
        os: Some(vec!["darwin".to_string()]),
        cpu: Some(vec!["arm64".to_string()]),
        libc: None,
    });
    let merged = cli.apply_to(existing).unwrap();
    assert_eq!(merged.os, Some(vec!["darwin".to_string()]));
    assert_eq!(merged.cpu, Some(vec!["x64".to_string()]));
    assert_eq!(merged.libc, None);
}

#[test]
fn cli_without_existing_creates_supported_architectures() {
    let cli = SupportedArchitecturesArgs {
        cpu: vec!["x64".to_string()],
        os: vec!["linux".to_string()],
        libc: vec!["glibc".to_string()],
    };
    let merged = cli.apply_to(None).unwrap();
    assert_eq!(merged.cpu, Some(vec!["x64".to_string()]));
    assert_eq!(merged.os, Some(vec!["linux".to_string()]));
    assert_eq!(merged.libc, Some(vec!["glibc".to_string()]));
}
