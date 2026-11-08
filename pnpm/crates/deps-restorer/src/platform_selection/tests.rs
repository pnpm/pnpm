use super::{VariantPlatforms, host_platform_selector, listed_link_platform};
use pnpm_lockfile::{
    Floor, LockfileResolution, PlatformAssetResolution, PlatformAssetTarget, RegistryResolution,
    VariationsResolution,
};
use pnpm_package_is_installable::{ArchitectureAxes, SupportedArchitectures};

const INTEGRITY: &str = "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";

fn listed(entries: &[&str]) -> SupportedArchitectures {
    SupportedArchitectures::Platforms(
        entries
            .iter()
            .map(|entry| entry.parse().expect("a platform entry"))
            .collect(),
    )
}

/// A platform no host running these tests is.
const FOREIGN: &str = "linux-s390x";

/// `@acme/cli` with an `any` archive and a `linux-s390x` variant built
/// against glibc 2.28.
fn floored_variations() -> VariationsResolution {
    let registry = || {
        LockfileResolution::Registry(RegistryResolution {
            integrity: INTEGRITY.parse().unwrap(),
            revision: None,
        })
    };
    VariationsResolution::new(vec![
        PlatformAssetResolution::any(registry()),
        PlatformAssetResolution {
            resolution: registry(),
            targets: vec![PlatformAssetTarget {
                os: "linux".into(),
                cpu: "s390x".into(),
                libc_version: Some(Floor { major: 2, minor: 28 }),
                ..PlatformAssetTarget::default()
            }],
        },
    ])
}

/// The tag of the archive `supported` links of [`floored_variations`].
fn linked_tag(supported: &SupportedArchitectures) -> String {
    let platforms = VariantPlatforms::of(Some(supported));
    let variations = floored_variations();
    variations
        .select(platforms.for_variations(&variations))
        .unwrap()
        .expect("an archive is linked")
        .variant_tag()
        .unwrap_or_else(|| "any".to_string())
}

#[test]
fn one_listed_platform_is_the_one_a_registry_package_links() {
    let platforms = VariantPlatforms::of(Some(&listed(&[FOREIGN])));
    assert_eq!((platforms.linked.os.as_str(), platforms.linked.cpu.as_str()), ("linux", "s390x"));
}

#[test]
fn a_list_with_current_or_several_platforms_links_the_hosts() {
    for supported in [listed(&["current", FOREIGN]), listed(&[FOREIGN, "darwin-arm64"])] {
        assert_eq!(listed_link_platform(Some(&supported)), None, "{supported:?}");
        assert_eq!(VariantPlatforms::of(Some(&supported)).linked, host_platform_selector());
    }
}

/// The `os`, `cpu`, and `libc` form names one platform the same way a list
/// does, its unset `libc` being the host's C library.
#[test]
fn the_axes_form_naming_one_platform_links_it() {
    let axes = ArchitectureAxes {
        os: Some(vec!["linux".into()]),
        cpu: Some(vec!["s390x".into()]),
        libc: None,
    };
    let platforms = VariantPlatforms::of(Some(&SupportedArchitectures::Axes(axes)));
    assert_eq!((platforms.linked.os.as_str(), platforms.linked.cpu.as_str()), ("linux", "s390x"));
    assert_eq!(
        linked_tag(&SupportedArchitectures::Axes(ArchitectureAxes {
            os: Some(vec!["linux".into()]),
            cpu: Some(vec!["s390x".into()]),
            libc: Some(vec!["glibc".into()]),
        })),
        "any",
        "an unstated floor is not met",
    );
}

#[test]
fn the_axes_form_with_current_an_unset_axis_or_several_platforms_links_the_hosts() {
    let axes = |os: Option<Vec<&str>>, cpu: Option<Vec<&str>>| {
        let owned = |values: Option<Vec<&str>>| {
            values.map(|values| {
                values
                    .into_iter()
                    .map(str::to_string)
                    .collect()
            })
        };
        SupportedArchitectures::Axes(ArchitectureAxes {
            os: owned(os),
            cpu: owned(cpu),
            libc: None,
        })
    };
    for supported in [
        axes(Some(vec!["linux"]), None),
        axes(None, Some(vec!["s390x"])),
        axes(Some(vec!["current"]), Some(vec!["s390x"])),
        axes(Some(vec!["linux"]), Some(vec!["s390x", "arm64"])),
    ] {
        assert_eq!(listed_link_platform(Some(&supported)), None, "{supported:?}");
    }
}

#[test]
fn a_variant_links_only_where_the_listed_platform_states_its_floor() {
    assert_eq!(linked_tag(&listed(&[FOREIGN])), "any");
    assert_eq!(linked_tag(&listed(&["linux-s390x-manylinux_2_28"])), "linux-s390x-manylinux_2_28");
}
