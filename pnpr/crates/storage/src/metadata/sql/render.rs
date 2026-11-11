//! Rendering a package view from its rows.
//!
//! Each ecosystem's document is assembled from normalized rows and the
//! mutable package state, and the assembled view is cached at the package's
//! generation. Every read checks that generation inside its read transaction,
//! so a cached view is served only while it is current: a lost invalidation
//! cannot pin an old view.

use super::{
    PackageKey,
    adapter::{Connection, Query, Row},
    cache::{FragmentKey, RENDERER_VERSION, RenderCache, Representation, ViewKey},
    cargo_rows,
    commit::{PackageRow, package_row_from, package_row_query, parse_json, parse_optional_json},
    oci_rows,
    packument::{Assembled, DEPRECATED, Interpretation, README, README_FILENAME, write_packument},
    pypi_rows,
    tags::tags_from_rows,
    to_column, to_generation,
};
use crate::metadata::{ARCHIVE_MANIFEST_FIELD, PackageView};
use pnpr_error::Result;
use pnpr_package_name::Ecosystem;
use serde_json::{Map, Value};
use std::sync::Arc;

enum ListedRows {
    Npm(Vec<Row>, Vec<Row>),
    Cargo(Vec<Row>),
    Pypi(Vec<Row>, Vec<Row>),
    Oci(Vec<Row>, Vec<Row>),
}

impl ListedRows {
    fn take(ecosystem: Ecosystem, results: &mut impl Iterator<Item = Vec<Row>>) -> Self {
        match ecosystem {
            Ecosystem::Npm => {
                Self::Npm(results.next().unwrap_or_default(), results.next().unwrap_or_default())
            }
            Ecosystem::Cargo => Self::Cargo(results.next().unwrap_or_default()),
            Ecosystem::Pypi => {
                Self::Pypi(results.next().unwrap_or_default(), results.next().unwrap_or_default())
            }
            Ecosystem::Oci => {
                Self::Oci(results.next().unwrap_or_default(), results.next().unwrap_or_default())
            }
        }
    }
}

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
    let cached = cache.view_generation(&view_key);
    let unless = cached.map_or(NO_GENERATION, to_column);
    let extras = extra.len();
    let reads = view_reads(key, &unless, extra);
    let mut results = snapshot.query_all(&reads)?.into_iter();
    let extra_rows: Vec<Vec<Row>> = results.by_ref().take(extras).collect();
    let Some(package) = package_row_from(key, results.next().unwrap_or_default())? else {
        return Ok((None, extra_rows));
    };
    let listed =
        (cached != Some(package.generation)).then(|| ListedRows::take(key.ecosystem, &mut results));
    let generation = key.generation(package.generation);
    let bytes = cached_or_rendered(snapshot, cache, view_key, &package, listed)?;
    Ok((Some(PackageView { bytes, generation }), extra_rows))
}

fn view_reads<'read>(
    key: &'read PackageKey,
    unless: &'read i64,
    mut extra: Vec<Query<'read>>,
) -> Vec<Query<'read>> {
    extra.push(package_row_query(key));
    match key.ecosystem {
        Ecosystem::Npm => {
            extra.extend([tags_query(key, unless), version_entries_query(key, unless)]);
        }
        Ecosystem::Cargo => extra.push(cargo_rows::view_query(key, unless)),
        Ecosystem::Pypi => extra.extend(pypi_rows::view_queries(key, unless)),
        Ecosystem::Oci => extra.extend(oci_rows::view_queries(key, unless)),
    }
    extra
}

fn cached_or_rendered(
    snapshot: &dyn Connection,
    cache: &RenderCache,
    view_key: ViewKey,
    package: &PackageRow,
    listed: Option<ListedRows>,
) -> Result<Arc<[u8]>> {
    if let Some(bytes) = cache.view(&view_key, package.generation) {
        return Ok(bytes);
    }
    let rendered = render_uncached(snapshot, cache, &view_key.package, package, listed)?;
    let bytes: Arc<[u8]> = rendered.into();
    cache.store_view(view_key, package.generation, Arc::clone(&bytes));
    Ok(bytes)
}

fn render_uncached(
    snapshot: &dyn Connection,
    cache: &RenderCache,
    key: &PackageKey,
    package: &PackageRow,
    listed: Option<ListedRows>,
) -> Result<Vec<u8>> {
    match key.ecosystem {
        Ecosystem::Npm => {
            let npm_listed = match listed {
                Some(ListedRows::Npm(tags, versions)) => Some((tags, versions)),
                _ => None,
            };
            assemble_packument(snapshot, cache, key, package, npm_listed)
        }
        Ecosystem::Cargo => {
            let cargo_listed = match listed {
                Some(ListedRows::Cargo(versions)) => Some(versions),
                _ => None,
            };
            cargo_rows::assemble(snapshot, key, package, cargo_listed, NO_GENERATION)
        }
        Ecosystem::Pypi => {
            let pypi_listed = match listed {
                Some(ListedRows::Pypi(files, removed)) => Some((files, removed)),
                _ => None,
            };
            pypi_rows::assemble(snapshot, key, package, pypi_listed, NO_GENERATION)
        }
        Ecosystem::Oci => {
            let oci_listed = match listed {
                Some(ListedRows::Oci(manifests, tags)) => Some((manifests, tags)),
                _ => None,
            };
            oci_rows::assemble(snapshot, key, package, oci_listed, NO_GENERATION)
        }
    }
}

/// One version as the version list reads it, before its fragment.
struct VersionEntry {
    version: String,
    published_at: Option<String>,
    fragment: FragmentKey,
}

fn version_entries_query<'query>(key: &'query PackageKey, unless: &'query i64) -> Query<'query> {
    let (catalog, ecosystem, name) = key.params();
    query!(
        "SELECT v.version, v.published_at, a.generation, m.profile
         FROM versions v
         JOIN accepted_manifests m USING (catalog, ecosystem, name, version)
         LEFT JOIN annotations a USING (catalog, ecosystem, name, version)
         WHERE v.catalog = ?1 AND v.ecosystem = ?2 AND v.name = ?3
           AND NOT EXISTS (
             SELECT 1 FROM packages p
             WHERE p.catalog = ?1 AND p.ecosystem = ?2 AND p.name = ?3 AND p.generation = ?4)
         ORDER BY v.version",
        catalog,
        ecosystem,
        name,
        unless,
    )
}

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
                profile: row.get(3)?,
                representation: Representation::Full,
                renderer: RENDERER_VERSION,
                annotation_generation: row.get::<Option<i64>>(2)?.map(to_generation),
            };
            Ok(VersionEntry {
                version,
                published_at: row.get::<Option<i64>>(1)?.map(crate::publish::iso_from_unix_millis),
                fragment,
            })
        })
        .collect()
}

pub(super) fn read_version(
    snapshot: &dyn Connection,
    cache: &RenderCache,
    key: &PackageKey,
    version: &str,
) -> Result<Option<Arc<str>>> {
    let (catalog, ecosystem, name) = key.params();
    let row = snapshot.query_optional(
        "SELECT profile FROM accepted_manifests
         WHERE catalog = ?1 AND ecosystem = ?2 AND name = ?3 AND version = ?4",
        params![catalog, ecosystem, name, version],
    )?;
    let Some(mut row) = row else { return Ok(None) };
    let fragment_key = FragmentKey {
        package: key.clone(),
        version: version.to_string(),
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

fn fragment_query(key: &FragmentKey) -> Query<'_> {
    let (catalog, ecosystem, name) = key.package.params();
    if key.representation == Representation::Accepted {
        return query!(
            "SELECT manifest, inputs, NULL AS readme, NULL AS readme_filename, NULL AS deprecated FROM accepted_manifests
             WHERE catalog = ?1 AND ecosystem = ?2 AND name = ?3 AND version = ?4
               AND profile = ?5",
            catalog,
            ecosystem,
            name,
            &key.version,
            &key.profile,
        );
    }
    query!(
        "SELECT m.manifest, m.inputs, r.readme, r.readme_filename, a.deprecated
         FROM accepted_manifests m
         LEFT JOIN readmes r USING (catalog, ecosystem, name, version)
         LEFT JOIN annotations a USING (catalog, ecosystem, name, version)
         WHERE m.catalog = ?1 AND m.ecosystem = ?2 AND m.name = ?3 AND m.version = ?4
           AND m.profile = ?5",
        catalog,
        ecosystem,
        name,
        &key.version,
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

const FRAGMENTS_PER_BATCH: usize = 256;

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
