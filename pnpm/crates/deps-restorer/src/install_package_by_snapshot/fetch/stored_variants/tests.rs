use crate::platform_selection::listed_platforms as fetch_platforms;
use pnpm_lockfile::{Floor, NodeAbi};
use pnpm_package_is_installable::{ArchitectureAxes, SupportedArchitectures};

#[test]
fn no_supported_architectures_stores_nothing_beside_the_linked_archive() {
    assert!(fetch_platforms(None).is_empty());
}

fn listed(entries: &[&str]) -> SupportedArchitectures {
    SupportedArchitectures::Platforms(
        entries
            .iter()
            .map(|entry| entry.parse().expect("a platform entry"))
            .collect(),
    )
}

/// A listed platform states its glibc floor by its `manylinux` baseline.
#[test]
fn a_listed_platform_states_its_glibc_floor() {
    let platforms = fetch_platforms(Some(&listed(&["linux-ppc64le-manylinux_2_26"])));

    assert_eq!(platforms.len(), 1);
    assert_eq!((platforms[0].os.as_str(), platforms[0].cpu.as_str()), ("linux", "ppc64"));
    let floors = platforms[0].floors.expect("another platform's floors are stated");
    assert_eq!(floors.libc_version, Some(Floor { major: 2, minor: 26 }));
    assert_eq!(floors.napi, None);
}

#[test]
fn the_axes_form_stands_for_every_combination() {
    let axes = ArchitectureAxes {
        os: Some(vec!["linux".into(), "darwin".into()]),
        cpu: Some(vec!["arm64".into()]),
        libc: Some(vec!["musl".into()]),
    };

    let platforms = fetch_platforms(Some(&SupportedArchitectures::Axes(axes)));

    let named: Vec<(&str, &str, Option<&str>)> = platforms
        .iter()
        .map(|platform| (platform.os.as_str(), platform.cpu.as_str(), platform.libc.as_deref()))
        .collect();
    assert_eq!(named, [("linux", "arm64", Some("musl")), ("darwin", "arm64", None)]);
}

/// A `libc` value of the axes form states its glibc floor as a listed
/// platform does.
#[test]
fn the_axes_form_states_a_glibc_floor() {
    let axes = ArchitectureAxes {
        os: Some(vec!["linux".into()]),
        cpu: Some(vec!["ppc64".into()]),
        libc: Some(vec!["manylinux_2_26".into(), "musllinux_1_2".into()]),
    };

    let platforms = fetch_platforms(Some(&SupportedArchitectures::Axes(axes)));

    let named: Vec<(Option<&str>, Option<Floor>)> = platforms
        .iter()
        .map(|platform| {
            (platform.libc.as_deref(), platform.floors.and_then(|floors| floors.libc_version))
        })
        .collect();
    assert_eq!(named, [(Some("glibc"), Some(Floor { major: 2, minor: 26 })), (Some("musl"), None)]);
}

#[test]
fn the_axes_form_reads_platform_aliases() {
    let axes = ArchitectureAxes {
        os: Some(vec!["linux".into()]),
        cpu: Some(vec!["aarch64".into()]),
        libc: Some(vec!["musllinux_1_2".into()]),
    };

    let platforms = fetch_platforms(Some(&SupportedArchitectures::Axes(axes)));

    let named: Vec<(&str, Option<&str>)> = platforms
        .iter()
        .map(|platform| (platform.cpu.as_str(), platform.libc.as_deref()))
        .collect();
    assert_eq!(named, [("arm64", Some("musl"))]);
}

#[test]
fn a_listed_platform_states_its_napi_and_node_abi_floors() {
    let platforms = fetch_platforms(Some(&listed(&["linux-arm64-napi8", "darwin-arm64-node127"])));

    let floors: Vec<(Option<u32>, Option<NodeAbi>)> = platforms
        .iter()
        .map(|platform| {
            let floors = platform.floors.expect("another platform's floors are stated");
            (floors.napi, floors.node_abi)
        })
        .collect();
    assert_eq!(floors, [(Some(8), None), (None, Some(NodeAbi::Node(127)))]);
}
