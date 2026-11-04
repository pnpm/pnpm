use super::{
    LockfileFormOptions, LockfileResolution, PlatformAssetResolution, RegistryResolution,
    RegistryServerType, TarballResolution, TarballRevision, VariationsResolution, assert_eq,
    integrity, render_resolution, revision_registry_tarball_url, selector, target,
};
use crate::variant_registry_tarball_url;

const REGISTRY: &str = "https://pnpr.example/";
const ANY: &str = "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";
const MUSL: &str = "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==";
const DARWIN: &str = "sha512-CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC==";
const DARWIN_URL: &str = "https://cdn.example/widget-darwin-arm64.tgz";
/// The `any` archive's URL on its revision route, `<name>@<version>+r2`.
const ANY_URL: &str = "https://pnpr.example/@acme/widget@1.2.3+r2";

fn revision() -> TarballRevision {
    TarballRevision::try_from(2).unwrap()
}

fn tarball(url: String, digest: &str) -> LockfileResolution {
    revised_tarball(url, digest, None)
}

fn revised_tarball(
    url: String,
    digest: &str,
    revision: Option<TarballRevision>,
) -> LockfileResolution {
    LockfileResolution::Tarball(TarballResolution {
        tarball: url,
        integrity: Some(integrity(digest)),
        revision,
        git_hosted: None,
        path: None,
    })
}

fn registry(digest: &str) -> LockfileResolution {
    LockfileResolution::Registry(RegistryResolution {
        integrity: integrity(digest),
        revision: None,
    })
}

/// The resolved archives of `@acme/widget@1.2.3`, revision 2: the `any`
/// archive on its revision route and a musl variant on its variant route, and a darwin
/// variant at a URL no route reconstructs.
fn resolved() -> VariationsResolution {
    let musl = PlatformAssetResolution {
        resolution: registry(MUSL),
        targets: vec![target("linux", "x64", Some("musl"))],
    };
    let musl_tag = musl.variant_tag().unwrap();
    let musl_url =
        variant_registry_tarball_url("@acme/widget", "1.2.3", &musl_tag, Some(2), REGISTRY)
            .unwrap();
    let mut variations = VariationsResolution::new(vec![
        PlatformAssetResolution::any(revised_tarball(ANY_URL.to_string(), ANY, Some(revision()))),
        PlatformAssetResolution { resolution: tarball(musl_url, MUSL), ..musl },
        PlatformAssetResolution {
            resolution: tarball(DARWIN_URL.to_string(), DARWIN),
            targets: vec![target("darwin", "arm64", None)],
        },
    ]);
    variations.revision = Some(2);
    variations
}

fn selected(variations: &VariationsResolution, os: &str, cpu: &str, libc: Option<&str>) -> String {
    let Ok(Some(variant)) = variations.select(&selector(os, cpu, libc)) else {
        panic!("{os}-{cpu} selects an archive");
    };
    variant.resolution
        .integrity()
        .unwrap()
        .to_string()
}

/// A package's archives written to the lockfile keep each archive's
/// integrity, the revision, and the URL of the archive no route rebuilds,
/// read back the same, and select the same archive for each host.
#[test]
fn variations_round_trip_through_the_lockfile() {
    let form = LockfileFormOptions {
        registry: REGISTRY,
        server_type: Some(RegistryServerType::Pnpr),
        include_tarball_url: false,
    };
    let written = LockfileResolution::Variations(resolved())
        .to_lockfile_form("@acme/widget", "1.2.3", form)
        .unwrap();
    let LockfileResolution::Variations(variations) = &written else {
        panic!("the archives stay variations");
    };
    assert_eq!(variations.revision, Some(2));
    assert_eq!(
        variations.variants[0].resolution,
        LockfileResolution::Registry(RegistryResolution {
            integrity: integrity(ANY),
            revision: Some(revision()),
        }),
    );
    assert_eq!(variations.variants[1].resolution, registry(MUSL));
    assert_eq!(variations.variants[2].resolution, tarball(DARWIN_URL.to_string(), DARWIN));

    let yaml = render_resolution(&written);
    let yaml = yaml.strip_prefix("resolution:").unwrap();
    let read: LockfileResolution = serde_saphyr::from_str(&textwrap_dedent(yaml)).unwrap();
    assert_eq!(read, written);

    let LockfileResolution::Variations(read) = &read else {
        panic!("the archives read back as variations");
    };
    assert_eq!(selected(read, "linux", "x64", Some("musl")), MUSL);
    assert_eq!(selected(read, "darwin", "arm64", None), DARWIN);
    assert_eq!(selected(read, "freebsd", "x64", None), ANY);
    let Some(LockfileResolution::Registry(any)) =
        read.any_variant().map(|variant| &variant.resolution)
    else {
        panic!("the any archive reads back in the registry form");
    };
    let any_revision = any.revision.expect("the any archive keeps its revision");
    assert_eq!(
        revision_registry_tarball_url("@acme/widget", "1.2.3", any_revision, REGISTRY),
        Some(ANY_URL.to_string()),
    );
}

/// Drop the indentation every line of `text` shares.
fn textwrap_dedent(text: &str) -> String {
    let indent = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start().len())
        .min()
        .unwrap_or(0);
    text.lines()
        .map(|line| {
            line.get(indent..)
                .unwrap_or_else(|| line.trim_start())
        })
        .collect::<Vec<_>>()
        .join("\n")
}
