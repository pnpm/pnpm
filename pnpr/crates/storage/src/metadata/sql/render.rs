//! Rendering a package view from its rows.
//!
//! An npm packument is assembled from cached per-version fragments and the
//! mutable package state, and the assembled view is cached at the package's
//! generation. Every read checks that generation inside its read transaction,
//! so a cached view is served only while it is current: a lost invalidation
//! cannot pin an old view. Reads on different connections share
//! the caches; two that miss one view at once may both render it.

use super::{
    PackageKey,
    adapter::{Connection, Query, Row},
    cache::{FragmentKey, RENDERER_VERSION, RenderCache, Representation, ViewKey},
    commit::{PackageRow, package_row_from, package_row_query, parse_json, parse_optional_json},
    packument::{
        DEPRECATED, Interpretation, ORDINARY, README, README_FILENAME, TAGS, TIME, VERSIONS,
    },
    tags::tags_from_rows,
    to_column, to_generation,
};
use crate::metadata::{ARCHIVE_MANIFEST_FIELD, PackageView};
use pnpr_error::Result;
use pnpr_package_name::Ecosystem;
use serde_json::{Map, Value};
use std::{collections::BTreeMap, sync::Arc};

pub(super) fn read_view(
    snapshot: &dyn Connection,
    cache: &RenderCache,
    key: &PackageKey,
) -> Result<Option<PackageView>> {
    Ok(read_view_with(snapshot, cache, key, Vec::new())?.0)
}

/// [`read_view`], with `extra` read before the view in the same round trip;
/// the rows of each are returned with it.
pub(super) fn read_view_with<'read>(
    snapshot: &dyn Connection,
    cache: &RenderCache,
    key: &'read PackageKey,
    extra: Vec<Query<'read>>,
) -> Result<(Option<PackageView>, Vec<Vec<Row>>)> {
    let view_key = ViewKey { package: key.clone(), representation: Representation::Full };
    // The rows a view is rendered from are read with the package's own row,
    // in one round trip to a remote database, unless the package is still
    // at the generation of the view the cache holds.
    let cached = cache.view_generation(&view_key);
    let unless = cached.map_or(NO_GENERATION, to_column);
    let extras = extra.len();
    let reads = view_reads(key, &unless, extra);
    let mut results = snapshot.query_all(&reads)?.into_iter();
    let extra_rows: Vec<Vec<Row>> = results.by_ref().take(extras).collect();
    let Some(package) = package_row_from(results.next().unwrap_or_default())? else {
        return Ok((None, extra_rows));
    };
    // The rows of the tags and versions were read unless the cached view was
    // at the package's generation.
    let listed =
        (key.ecosystem == Ecosystem::Npm && cached != Some(package.generation)).then(|| {
            (results.next().unwrap_or_default(), results.next().unwrap_or_default())
        });
    let generation = key.generation(package.generation);
    let bytes = cached_or_rendered(snapshot, cache, view_key, package, listed)?;
    Ok((Some(PackageView { bytes, generation }), extra_rows))
}

/// `extra`, then the reads a view of `key` is rendered from: its package
/// row, and for npm its tags and versions unless the cached view is at
/// generation `unless`.
fn view_reads<'read>(
    key: &'read PackageKey,
    unless: &'read i64,
    mut extra: Vec<Query<'read>>,
) -> Vec<Query<'read>> {
    extra.push(package_row_query(key));
    if key.ecosystem == Ecosystem::Npm {
        extra.extend([tags_query(key, unless), version_entries_query(key, unless)]);
    }
    extra
}

/// The view `view_key` names at `package`'s generation, from the cache or
/// rendered from `package` and the `listed` tag and version rows.
fn cached_or_rendered(
    snapshot: &dyn Connection,
    cache: &RenderCache,
    view_key: ViewKey,
    package: PackageRow,
    listed: Option<(Vec<Row>, Vec<Row>)>,
) -> Result<Arc<[u8]>> {
    if let Some(bytes) = cache.view(&view_key, package.generation) {
        return Ok(bytes);
    }
    let key = &view_key.package;
    let generation = package.generation;
    let bytes: Arc<[u8]> = match key.ecosystem {
        Ecosystem::Npm => assemble_packument(snapshot, cache, key, &package, listed)?.into(),
        Ecosystem::Cargo | Ecosystem::Pypi | Ecosystem::Oci => package.state.into(),
    };
    cache.store_view(view_key, generation, Arc::clone(&bytes));
    Ok(bytes)
}

/// One version as the version list reads it, before its fragment.
struct VersionEntry {
    version: String,
    published_at: Option<String>,
    fragment: FragmentKey,
}

/// The read of a package's versions, unless the package is at generation
/// `unless`.
fn version_entries_query<'query>(key: &'query PackageKey, unless: &'query i64) -> Query<'query> {
    let (catalog, ecosystem, name) = key.params();
    query!(
        "SELECT v.version, v.published_at, a.generation, m.profile
         FROM versions v
         JOIN accepted_manifests m USING (catalog, ecosystem, name, version, revision_key)
         LEFT JOIN annotations a USING (catalog, ecosystem, name, version)
         WHERE v.catalog = ?1 AND v.ecosystem = ?2 AND v.name = ?3 AND v.revision_key = ?4
           AND NOT EXISTS (
             SELECT 1 FROM packages p
             WHERE p.catalog = ?1 AND p.ecosystem = ?2 AND p.name = ?3 AND p.generation = ?5)
         ORDER BY v.version",
        catalog,
        ecosystem,
        name,
        ORDINARY,
        unless,
    )
}

/// The read of a package's tags, unless the package is at generation
/// `unless`.
fn tags_query<'query>(key: &'query PackageKey, unless: &'query i64) -> Query<'query> {
    let (catalog, ecosystem, name) = key.params();
    query!(
        "SELECT t.tag, t.version FROM tags t
         WHERE t.catalog = ?1 AND t.ecosystem = ?2 AND t.name = ?3
           AND NOT EXISTS (
             SELECT 1 FROM packages p
             WHERE p.catalog = ?1 AND p.ecosystem = ?2 AND p.name = ?3 AND p.generation = ?4)",
        catalog,
        ecosystem,
        name,
        unless,
    )
}

/// What a read of a package at generation `unless` excludes when the
/// package is at none: every generation is positive.
const NO_GENERATION: i64 = -1;

fn version_entries_from(key: &PackageKey, rows: Vec<Row>) -> Result<Vec<VersionEntry>> {
    rows.into_iter()
        .map(|mut row| {
            let version: String = row.get(0)?;
            let fragment = FragmentKey {
                package: key.clone(),
                version: version.clone(),
                revision_key: ORDINARY.to_string(),
                profile: row.get(3)?,
                representation: Representation::Full,
                renderer: RENDERER_VERSION,
                annotation_generation: row.get::<Option<i64>>(2)?.map(to_generation),
            };
            Ok(VersionEntry { version, published_at: row.get(1)?, fragment })
        })
        .collect()
}

/// One version's accepted manifest with its interpretation, without its
/// README and deprecation, or `None` when the package has no such version.
/// Only the version's rows are read: the package view is neither assembled
/// nor parsed.
pub(super) fn read_version(
    snapshot: &dyn Connection,
    cache: &RenderCache,
    key: &PackageKey,
    version: &str,
) -> Result<Option<Arc<str>>> {
    let (catalog, ecosystem, name) = key.params();
    let row = snapshot.query_optional(
        "SELECT profile FROM accepted_manifests
         WHERE catalog = ?1 AND ecosystem = ?2 AND name = ?3 AND version = ?4
           AND revision_key = ?5",
        params![catalog, ecosystem, name, version, ORDINARY],
    )?;
    let Some(mut row) = row else { return Ok(None) };
    let fragment_key = FragmentKey {
        package: key.clone(),
        version: version.to_string(),
        revision_key: ORDINARY.to_string(),
        profile: row.get(0)?,
        representation: Representation::Accepted,
        renderer: RENDERER_VERSION,
        annotation_generation: None,
    };
    if let Some(fragment) = cache.fragment(&fragment_key) {
        return Ok(Some(fragment));
    }
    let rows = snapshot.query_all(&[fragment_query(&fragment_key)])?;
    let fragment = render_fragment(
        &fragment_key,
        rows.into_iter()
            .next()
            .unwrap_or_default(),
    )?;
    cache.store_fragment(fragment_key, Arc::clone(&fragment));
    Ok(Some(fragment))
}

/// Render one version's manifest with its interpretation, and for the full
/// representation its README and deprecation, from the row of
/// [`fragment_query`].
fn render_fragment(key: &FragmentKey, rows: Vec<Row>) -> Result<Arc<str>> {
    let [manifest, inputs, fields @ ..] = fragment_columns(rows)?;
    let mut fragment: Map<String, Value> = parse_json(manifest.as_deref().unwrap_or("{}"))?;
    let interpretation = Interpretation {
        profile: key.profile.clone(),
        inputs: parse_json(inputs.as_deref().unwrap_or("{}"))?,
    };
    if let Some(record) = interpretation.record() {
        fragment.insert(ARCHIVE_MANIFEST_FIELD.to_string(), record);
    }
    if key.representation == Representation::Accepted {
        return Ok(serde_json::to_string(&fragment)?.into());
    }
    for (field, value) in [README, README_FILENAME, DEPRECATED].into_iter().zip(fields) {
        if let Some(value) = parse_optional_json(value.as_deref())? {
            fragment.insert(field.to_string(), value);
        }
    }
    Ok(serde_json::to_string(&fragment)?.into())
}

/// The read of a version's manifest, interpretation inputs, README, README
/// filename, and deprecation, as stored. The accepted representation reads
/// only the first two.
fn fragment_query(key: &FragmentKey) -> Query<'_> {
    let (catalog, ecosystem, name) = key.package.params();
    if key.representation == Representation::Accepted {
        return query!(
            "SELECT manifest, inputs, NULL, NULL, NULL FROM accepted_manifests
             WHERE catalog = ?1 AND ecosystem = ?2 AND name = ?3 AND version = ?4
               AND revision_key = ?5 AND profile = ?6",
            catalog,
            ecosystem,
            name,
            &key.version,
            &key.revision_key,
            &key.profile,
        );
    }
    query!(
        "SELECT m.manifest, m.inputs, r.readme, r.readme_filename, a.deprecated
         FROM accepted_manifests m
         LEFT JOIN readmes r USING (catalog, ecosystem, name, version, revision_key)
         LEFT JOIN annotations a USING (catalog, ecosystem, name, version)
         WHERE m.catalog = ?1 AND m.ecosystem = ?2 AND m.name = ?3 AND m.version = ?4
           AND m.revision_key = ?5 AND m.profile = ?6",
        catalog,
        ecosystem,
        name,
        &key.version,
        &key.revision_key,
        &key.profile,
    )
}

fn fragment_columns(rows: Vec<Row>) -> Result<[Option<String>; 5]> {
    let mut row = rows
        .into_iter()
        .next()
        .ok_or_else(|| pnpr_error::RegistryError::Internal {
            reason: "a version listed for a package has no accepted manifest".to_string(),
        })?;
    Ok([row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?])
}

/// The fragment of each of `entries`, from the cache or rendered from rows
/// read in one batch, which the cache then keeps.
fn fragments(
    connection: &dyn Connection,
    cache: &RenderCache,
    entries: &[VersionEntry],
) -> Result<Vec<Arc<str>>> {
    let mut fragments: Vec<Option<Arc<str>>> = entries
        .iter()
        .map(|entry| cache.fragment(&entry.fragment))
        .collect();
    let missing: Vec<usize> = (0..entries.len())
        .filter(|&index| fragments[index].is_none())
        .collect();
    for chunk in missing.chunks(FRAGMENTS_PER_BATCH) {
        let queries: Vec<Query<'_>> = chunk
            .iter()
            .map(|&index| fragment_query(&entries[index].fragment))
            .collect();
        for (&index, rows) in chunk
            .iter()
            .zip(connection.query_all(&queries)?)
        {
            let key = &entries[index].fragment;
            let fragment = render_fragment(key, rows)?;
            cache.store_fragment(key.clone(), Arc::clone(&fragment));
            fragments[index] = Some(fragment);
        }
    }
    Ok(fragments
        .into_iter()
        .flatten()
        .collect())
}

/// How many fragments one batch reads, so a package with many versions and
/// a cold cache does not read them all in one response.
const FRAGMENTS_PER_BATCH: usize = 256;

/// The parts of a packument that are not package state.
struct Assembled {
    tags: BTreeMap<String, String>,
    versions: Vec<(String, Arc<str>)>,
    /// `time` entries of the versions, in version order.
    published: Vec<(String, String)>,
}

/// Assemble the packument of `package` from its rows. `listed` holds the
/// rows of its tags and of its version list when they were read with the
/// package's own.
fn assemble_packument(
    connection: &dyn Connection,
    cache: &RenderCache,
    key: &PackageKey,
    package: &PackageRow,
    listed: Option<(Vec<Row>, Vec<Row>)>,
) -> Result<Vec<u8>> {
    let state: Map<String, Value> = serde_json::from_slice(&package.state)?;
    let (tags, entries) = if let Some(listed) = listed {
        listed
    } else {
        let reads = [tags_query(key, &NO_GENERATION), version_entries_query(key, &NO_GENERATION)];
        let mut results = connection.query_all(&reads)?.into_iter();
        let tags = results.next().unwrap_or_default();
        (tags, results.next().unwrap_or_default())
    };
    let tags = tags_from_rows(tags)?;
    let entries = version_entries_from(key, entries)?;
    let fragments = fragments(connection, cache, &entries)?;
    let mut assembled = Assembled { tags, versions: Vec::new(), published: Vec::new() };
    for (entry, fragment) in entries.into_iter().zip(fragments) {
        if let Some(published_at) = entry.published_at {
            assembled.published.push((entry.version.clone(), published_at));
        }
        assembled.versions.push((entry.version, fragment));
    }
    write_packument(&state, &assembled)
}

/// Serialize the package state with the assembled parts in their places.
/// Fragments are copied in as they were rendered, never parsed again.
fn write_packument(state: &Map<String, Value>, assembled: &Assembled) -> Result<Vec<u8>> {
    let fragment_bytes: usize = assembled.versions
        .iter()
        .map(|(_, fragment)| fragment.len())
        .sum();
    let mut out = Vec::with_capacity(fragment_bytes + 1024);
    out.push(b'{');
    let missing = [TAGS, VERSIONS]
        .into_iter()
        .filter(|field| !state.contains_key(*field));
    let fields = state
        .iter()
        .map(|(field, value)| (field.as_str(), value));
    for (index, (field, stored)) in fields
        .chain(missing.map(|field| (field, &Value::Null)))
        .enumerate()
    {
        if index > 0 {
            out.push(b',');
        }
        serde_json::to_writer(&mut out, field)?;
        out.push(b':');
        write_placed(&mut out, field, stored, assembled)?;
    }
    out.push(b'}');
    Ok(out)
}

/// Write the value of `field`: an assembled part, or the stored state.
fn write_placed(
    out: &mut Vec<u8>,
    field: &str,
    stored: &Value,
    assembled: &Assembled,
) -> Result<()> {
    match field {
        TAGS => serde_json::to_writer(out, &assembled.tags)?,
        VERSIONS => {
            out.push(b'{');
            for (index, (version, fragment)) in assembled.versions.iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                serde_json::to_writer(&mut *out, version)?;
                out.push(b':');
                out.extend_from_slice(fragment.as_bytes());
            }
            out.push(b'}');
        }
        TIME => {
            let mut time = stored
                .as_object()
                .cloned()
                .unwrap_or_default();
            for (version, published_at) in &assembled.published {
                time.insert(version.clone(), Value::String(published_at.clone()));
            }
            serde_json::to_writer(out, &time)?;
        }
        _ => serde_json::to_writer(out, stored)?,
    }
    Ok(())
}
