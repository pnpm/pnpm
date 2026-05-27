use super::{
    archive_filter_for, emit_progress_resolved, host_platform_selector, node_extras_filter,
    render_variant_targets, synthesize_runtime_manifest_bytes,
};
use pacquet_graph_hasher::{host_arch, host_libc, host_platform};
use pacquet_lockfile::{
    BinaryArchive, BinaryResolution, BinarySpec, LockfileResolution, PackageKey,
    PlatformAssetResolution, PlatformAssetTarget,
};
use pacquet_reporter::{LogEvent, ProgressMessage, Reporter};
use pretty_assertions::assert_eq;
use std::sync::Mutex;

/// `emit_progress_resolved` fires exactly one `pnpm:progress`
/// `resolved` event with the supplied (`package_id`, `requester`).
/// The pair pins pnpm's per-package counter to the right row.
#[test]
fn emits_resolved_with_supplied_identifiers() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());

    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    EVENTS.lock().unwrap().clear();
    emit_progress_resolved::<RecordingReporter>("react@18.0.0", "/proj");

    let captured = EVENTS.lock().unwrap();
    assert!(
        matches!(
            captured.as_slice(),
            [LogEvent::Progress(log)] if matches!(
                &log.message,
                ProgressMessage::Resolved { package_id, requester }
                    if package_id == "react@18.0.0" && requester == "/proj",
            ),
        ),
        "expected a single Resolved event with matching identifiers; got {captured:?}",
    );
}

/// `host_platform_selector` builds the selector that drives runtime-
/// variant matching. The `os` / `cpu` fields are always populated
/// (from `host_platform()` / `host_arch()`); `libc` is the
/// interesting one — pacquet must translate the
/// "non-Linux ⇒ no libc constraint" rule pnpm enforces:
/// `process.platform === 'linux' ? family : null`.
///
/// Asserting platform-specific shape directly would mean four
/// `cfg`-gated tests; instead, run the live `host_*` functions and
/// pin the *relationship* — `host_libc() == "unknown"` iff the
/// selector's `libc` field is `None`. The relationship covers both
/// the macOS / Windows / BSD non-Linux case (`libc` always `None`)
/// and the Linux case (`libc` always `Some("glibc")` /
/// `Some("musl")`).
#[test]
fn host_platform_selector_omits_libc_on_non_linux_hosts() {
    let selector = host_platform_selector();
    let libc_known = host_libc() != "unknown";
    assert_eq!(selector.os, host_platform());
    assert_eq!(selector.cpu, host_arch());
    assert_eq!(
        selector.libc.is_some(),
        libc_known,
        "selector.libc should be Some iff host_libc() reports glibc/musl (Linux); got selector={selector:?}, host_libc={:?}",
        host_libc(),
    );
    if libc_known {
        assert_eq!(selector.libc.as_deref(), Some(host_libc()));
    }
}

/// `render_variant_targets` renders the lockfile's advertised
/// target triples for inclusion in the
/// `NoMatchingPlatformVariant` error message. Each target lands as
/// `os/cpu` with an optional `+libc` suffix, joined with `, ` so
/// the rendered list is greppable from terminal output.
#[test]
fn render_variant_targets_formats_each_triple_with_optional_libc() {
    let variants = vec![
        PlatformAssetResolution {
            // Inner resolution is unused by the renderer; pick any
            // shape that round-trips through serde (Directory keeps
            // the fixture light).
            resolution: LockfileResolution::Directory(pacquet_lockfile::DirectoryResolution {
                directory: "fixture".into(),
            }),
            targets: vec![
                PlatformAssetTarget { os: "darwin".into(), cpu: "arm64".into(), libc: None },
                PlatformAssetTarget {
                    os: "linux".into(),
                    cpu: "x64".into(),
                    libc: Some("musl".into()),
                },
            ],
        },
        PlatformAssetResolution {
            resolution: LockfileResolution::Directory(pacquet_lockfile::DirectoryResolution {
                directory: "fixture".into(),
            }),
            targets: vec![PlatformAssetTarget {
                os: "win32".into(),
                cpu: "x64".into(),
                libc: None,
            }],
        },
    ];

    let rendered = render_variant_targets(&variants);
    assert_eq!(rendered, "darwin/arm64, linux/x64+musl, win32/x64");
}

/// `node_extras_filter` is the hand-coded port of upstream's
/// `NODE_EXTRAS_IGNORE_PATTERN` regex
/// (`^(?:(?:lib/)?node_modules/(?:npm|corepack)(?:/|$)|bin/(?:npm|npx|corepack)$|(?:npm|npx|corepack)(?:\.(?:cmd|ps1))?$)`).
/// Pin each branch of the alternation, including the negative
/// cases the regex deliberately doesn't match — a regression
/// (e.g. matching `lib/node_modules/yarn/...` because someone
/// forgot the `npm|corepack` alternation) would slip past tests
/// that only checked positive matches.
#[test]
fn node_extras_filter_matches_upstream_regex_alternations() {
    // Branch 1: `^(?:lib/)?node_modules/(?:npm|corepack)(?:/|$)`
    for path in [
        "lib/node_modules/npm",
        "lib/node_modules/npm/",
        "lib/node_modules/npm/package.json",
        "lib/node_modules/corepack",
        "lib/node_modules/corepack/dist/manager.js",
        "node_modules/npm",
        "node_modules/npm/package.json",
        "node_modules/corepack/dist/manager.js",
    ] {
        assert!(node_extras_filter(path), "expected match: {path}");
    }
    // Same branch, negative cases: other package names under
    // `node_modules/` must not be stripped.
    for path in [
        "lib/node_modules/yarn",
        "lib/node_modules/yarn/package.json",
        "node_modules/yarn",
        "node_modules/typescript/lib/tsc.js",
        // `node_modules` without the `lib/` prefix at a nested
        // depth shouldn't match the regex either (the `^` anchor).
        "src/node_modules/npm/foo",
    ] {
        assert!(!node_extras_filter(path), "expected no match: {path}");
    }

    // Branch 2: `^bin/(?:npm|npx|corepack)$`
    for path in ["bin/npm", "bin/npx", "bin/corepack"] {
        assert!(node_extras_filter(path), "expected match: {path}");
    }
    // Same branch, negative cases: the `$` anchors the regex —
    // `bin/npm/foo` doesn't match, neither does an extension on
    // the `bin/` form.
    for path in ["bin/npm/foo", "bin/npm.cmd", "bin/yarn", "bin/", "binnpm"] {
        assert!(!node_extras_filter(path), "expected no match: {path}");
    }

    // Branch 3: `^(?:npm|npx|corepack)(?:\.(?:cmd|ps1))?$`
    for path in [
        "npm",
        "npx",
        "corepack",
        "npm.cmd",
        "npx.cmd",
        "corepack.cmd",
        "npm.ps1",
        "npx.ps1",
        "corepack.ps1",
    ] {
        assert!(node_extras_filter(path), "expected match: {path}");
    }
    // Same branch, negative cases: unsupported extensions and
    // unrelated names at the root.
    for path in ["npm.bat", "npm.exe", "node", "yarn", "npmrc", "npm.cmd.bak"] {
        assert!(!node_extras_filter(path), "expected no match: {path}");
    }
}

/// `archive_filter_for` is the per-package filter dispatcher —
/// returns `Some(NODE_EXTRAS)` only for the unscoped `node`
/// package (mirroring upstream's `archiveFilters: { node: ... }`
/// keyed by `pkg.name`). `@foo/node` and any other package must
/// get `None` so the full archive contents land in the CAS
/// unfiltered.
#[test]
fn archive_filter_for_only_returns_filter_for_unscoped_node() {
    let key_node: PackageKey = "node@22.0.0".parse().expect("parse node key");
    assert!(archive_filter_for(&key_node).is_some(), "node must get the filter");

    let key_scoped_node: PackageKey = "@foo/node@22.0.0".parse().expect("parse @foo/node key");
    assert!(
        archive_filter_for(&key_scoped_node).is_none(),
        "scoped `@foo/node` must not get the filter; upstream `archiveFilters` is keyed by pkg.name and only matches the unscoped string `node`",
    );

    let key_react: PackageKey = "react@18.0.0".parse().expect("parse react key");
    assert!(archive_filter_for(&key_react).is_none());

    let key_bun: PackageKey = "bun@1.0.0".parse().expect("parse bun key");
    assert!(
        archive_filter_for(&key_bun).is_none(),
        "bun runtime has no bundled-tooling filter upstream (yet); leaving it `None` matches",
    );
}

/// `synthesize_runtime_manifest_bytes` is the
/// `appendManifest`-equivalent for the runtime fetcher: it writes a
/// `name` / `version` / `bin` JSON object into the CAS so the
/// existing bin-link step (which reads bins off the slot's
/// `package.json`) has something to consume. Pin the wire shape
/// for both `BinarySpec` variants (single string + map) so a
/// regression in either branch can't silently strip the bin field
/// downstream.
#[test]
fn synthesize_runtime_manifest_emits_name_version_and_bin_single() {
    let key: PackageKey = "node@22.0.0".parse().expect("parse node key");
    let binary = BinaryResolution {
        url: "https://nodejs.org/dist/v22.0.0/node-v22.0.0-darwin-arm64.tar.gz".to_string(),
        integrity: "sha512-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa==".parse().expect("parse integrity"),
        bin: BinarySpec::Single("bin/node".to_string()),
        archive: BinaryArchive::Tarball,
        prefix: None,
    };

    let bytes = synthesize_runtime_manifest_bytes(&key, &binary)
        .expect("synth must succeed for a well-formed BinarySpec::Single");
    let parsed: serde_json::Value =
        serde_json::from_slice(&bytes).expect("synth bytes must round-trip through serde_json");

    dbg!(&parsed);
    assert_eq!(parsed["name"], "node");
    assert_eq!(parsed["version"], "22.0.0");
    // `BinarySpec::Single` lands as a JSON string. pnpm's bin
    // resolver treats `bin: "bin/node"` as "one binary, named after
    // the package" — so the shim is `<modules_dir>/.bin/node` →
    // `<slot>/bin/node`. Preserve that exact shape.
    assert_eq!(parsed["bin"], "bin/node");
}

#[test]
fn synthesize_runtime_manifest_emits_name_version_and_bin_map() {
    let key: PackageKey = "node@22.0.0".parse().expect("parse node key");
    let mut bin_map = std::collections::BTreeMap::new();
    bin_map.insert("node".to_string(), "bin/node".to_string());
    bin_map.insert("node-mips".to_string(), "bin/node-mips".to_string());
    let binary = BinaryResolution {
        url: "https://nodejs.org/dist/v22.0.0/node-v22.0.0-darwin-arm64.tar.gz".to_string(),
        integrity: "sha512-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa==".parse().expect("parse integrity"),
        bin: BinarySpec::Map(bin_map),
        archive: BinaryArchive::Tarball,
        prefix: None,
    };

    let bytes = synthesize_runtime_manifest_bytes(&key, &binary)
        .expect("synth must succeed for a well-formed BinarySpec::Map");
    let parsed: serde_json::Value =
        serde_json::from_slice(&bytes).expect("synth bytes must round-trip");

    dbg!(&parsed);
    assert_eq!(parsed["name"], "node");
    assert_eq!(parsed["version"], "22.0.0");
    // `BinarySpec::Map` lands as a JSON object. Each entry pins
    // (bin_name → relative path); pnpm's bin resolver creates one
    // shim per entry under `<modules_dir>/.bin/<bin_name>`.
    assert_eq!(parsed["bin"]["node"], "bin/node");
    assert_eq!(parsed["bin"]["node-mips"], "bin/node-mips");
}

/// Scoped packages preserve the `@scope/name` form in the
/// synthesized manifest's `name` field — `PkgName`'s Display
/// already handles that, and the synth function passes the result
/// through verbatim. Future runtime entries could conceivably ship
/// scoped (e.g. `@deno/runtime`) so pin the shape now rather than
/// catch it later.
#[test]
fn synthesize_runtime_manifest_preserves_scoped_name() {
    let key: PackageKey = "@foo/bar@1.2.3".parse().expect("parse scoped key");
    let binary = BinaryResolution {
        url: "https://example.test/foo-bar-1.2.3.tar.gz".to_string(),
        integrity: "sha512-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa==".parse().expect("parse integrity"),
        bin: BinarySpec::Single("bin/bar".to_string()),
        archive: BinaryArchive::Tarball,
        prefix: None,
    };

    let bytes = synthesize_runtime_manifest_bytes(&key, &binary).expect("synth must succeed");
    let parsed: serde_json::Value = serde_json::from_slice(&bytes).expect("round-trip");

    assert_eq!(parsed["name"], "@foo/bar");
    assert_eq!(parsed["version"], "1.2.3");
}
