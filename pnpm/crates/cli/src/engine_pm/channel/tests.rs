use super::{BinaryChannel, Channel, PackageManager};

fn yarn_channel_of(version_spec: &str) -> Channel {
    PackageManager::Yarn.channel(version_spec)
}

fn registry_package(channel: Channel) -> &'static str {
    match channel {
        Channel::Registry { package } => package,
        Channel::Binary(binary) => panic!("expected a registry channel, got {binary:?}"),
    }
}

#[test]
fn parses_only_the_provisionable_package_managers() {
    for name in ["bun", "npm", "pnpm", "yarn"] {
        assert_eq!(PackageManager::parse(name).map(PackageManager::name), Some(name));
    }
    assert_eq!(PackageManager::parse("yarnpkg"), None);
    assert_eq!(PackageManager::parse("cnpm"), None);
    assert_eq!(PackageManager::parse(""), None);
}

#[test]
fn yarn_classic_specifiers_resolve_against_the_yarn_package() {
    for version_spec in ["1.22.22", "^1.22.0", "1", "~1.22", ">=1 <2", "0.27.5"] {
        assert_eq!(registry_package(yarn_channel_of(version_spec)), "yarn", "{version_spec}");
    }
}

#[test]
fn yarn_berry_specifiers_resolve_against_the_cli_dist_package() {
    for version_spec in ["4.9.2", "^4.0.0", "2", "3.6.4", ">=2", "5.0.0-rc.1"] {
        assert_eq!(
            registry_package(yarn_channel_of(version_spec)),
            "@yarnpkg/cli-dist",
            "{version_spec}",
        );
    }
}

#[test]
fn yarn_6_and_above_come_from_release_archives() {
    for version_spec in ["6", "^6.0.0", "6.0.0-rc.19", "7.1.0"] {
        assert_eq!(
            yarn_channel_of(version_spec),
            Channel::Binary(BinaryChannel::Yarn),
            "{version_spec}",
        );
    }
}

/// A specifier that names no major at all must not fall to Yarn 1 — a
/// dist-tag or wildcard means "the current line", which is Berry.
#[test]
fn uncommitted_yarn_specifiers_fall_to_the_current_line() {
    for version_spec in ["latest", "*", "x", "", "  ", "canary", "not a range"] {
        assert_eq!(
            registry_package(yarn_channel_of(version_spec)),
            "@yarnpkg/cli-dist",
            "{version_spec}",
        );
    }
}

#[test]
fn npm_and_pnpm_have_one_channel_each() {
    assert_eq!(PackageManager::Npm.channel("latest"), Channel::Registry { package: "npm" });
    assert_eq!(PackageManager::Pnpm.channel("11.0.0"), Channel::Registry { package: "pnpm" });
    assert_eq!(PackageManager::Bun.channel("1.2.0"), Channel::Binary(BinaryChannel::Bun));
}

#[test]
fn registry_engines_pin_their_own_package() {
    let npm = PackageManager::Npm.engine_packages("11.0.0").unwrap();
    assert_eq!((npm.wrapper, npm.pinned, npm.links_native_binary), ("npm", &["npm"][..], false));

    let classic = PackageManager::Yarn.engine_packages("1.22.22").unwrap();
    assert_eq!(classic.wrapper, "yarn");
    assert_eq!(classic.pinned, &["yarn"]);

    let berry = PackageManager::Yarn.engine_packages("4.9.2").unwrap();
    assert_eq!(berry.wrapper, "@yarnpkg/cli-dist");
    assert_eq!(berry.pinned, &["@yarnpkg/cli-dist"]);
}

/// pnpm's engine is the one whose package set changes with the version:
/// the JS CLI alone, the JS CLI plus `@pnpm/exe`, then the native `pnpm`.
#[test]
fn the_pnpm_engine_follows_its_own_packaging_history() {
    let legacy = PackageManager::Pnpm.engine_packages("6.0.0").unwrap();
    assert_eq!(legacy.wrapper, "pnpm");
    assert!(!legacy.links_native_binary);

    let with_exe = PackageManager::Pnpm.engine_packages("11.0.0").unwrap();
    assert_eq!(with_exe.wrapper, "@pnpm/exe");
    assert_eq!(with_exe.pinned, &["pnpm", "@pnpm/exe"]);
    assert!(with_exe.links_native_binary);

    let native = PackageManager::Pnpm.engine_packages("12.0.0").unwrap();
    assert_eq!(native.wrapper, "pnpm");
    assert_eq!(native.pinned, &["pnpm"]);
    assert!(native.links_native_binary);
}

#[test]
fn binary_engines_have_no_package_closure() {
    assert_eq!(PackageManager::Bun.engine_packages("1.2.0"), None);
    assert_eq!(PackageManager::Yarn.engine_packages("6.0.0-rc.19"), None);
}
