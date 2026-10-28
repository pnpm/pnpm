/// Pick the `package.json` fields downstream code actually reads — bin
/// linking, dependency resolution, build-script detection — and discard
/// the rest, keeping only the lifecycle hooks pnpm executes out
/// of `scripts`.
///
/// The subset exists to bound what lands in `index.db`: a full manifest
/// runs to tens of KB, and msgpackr-records tops out at `0x7f` record
/// slots.
///
/// `None` rather than an empty object when nothing survives, which
/// would otherwise round-trip as a zero-field record def.
#[must_use]
pub fn normalize_bundled_manifest(value: &serde_json::Value) -> Option<serde_json::Value> {
    /// Fields kept verbatim from the source manifest.
    ///
    /// Order matters for the on-wire byte sequence — msgpackr emits
    /// fields in JS object insertion order, and pacquet's encoder
    /// follows the [`serde_json::Map`] iteration order — but it
    /// does *not* matter for property-access correctness on the
    /// pnpm side. The order below matches the field order pnpm
    /// emits so a side-by-side byte diff against a pnpm-written
    /// row is shallower.
    const BUNDLED_MANIFEST_FIELDS: &[&str] = &[
        "bin",
        "bundledDependencies",
        "bundleDependencies",
        "cpu",
        "dependencies",
        "devDependencies",
        "directories",
        "engines",
        "gypfile",
        "libc",
        "name",
        "optionalDependencies",
        "os",
        "peerDependencies",
        "peerDependenciesMeta",
    ];
    const LIFECYCLE_SCRIPTS: &[&str] = &["preinstall", "install", "postinstall"];

    let serde_json::Value::Object(map) = value else { return None };
    let mut picked = serde_json::Map::new();

    // pnpm emits `version` first regardless of whether it was first
    // in the source object. Version normalization
    // via `semver.clean(...)` (pnpm only loose-cleans for the bundled
    // row, not for resolution) is intentionally skipped: the inputs
    // from a real npm tarball are already semver-clean in practice,
    // and pulling `node-semver` into this crate purely for
    // this normalization would carry more risk than the deviation it
    // closes.
    pick_fields(&mut picked, map, &["version"]);
    pick_fields(&mut picked, map, BUNDLED_MANIFEST_FIELDS);

    if let Some(serde_json::Value::Object(scripts)) = map.get("scripts") {
        let mut sub = serde_json::Map::new();
        pick_fields(&mut sub, scripts, LIFECYCLE_SCRIPTS);
        if !sub.is_empty() {
            picked.insert("scripts".to_string(), serde_json::Value::Object(sub));
        }
    }

    if picked.is_empty() { None } else { Some(serde_json::Value::Object(picked)) }
}

/// Copy the named fields that `source` actually carries, in the order given.
/// A null is treated as absent: pnpm's own row omits it.
fn pick_fields(
    picked: &mut serde_json::Map<String, serde_json::Value>,
    source: &serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
) {
    for &key in keys {
        if let Some(value) = source.get(key)
            && !value.is_null()
        {
            picked.insert(key.to_string(), value.clone());
        }
    }
}
