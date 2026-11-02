//! Rendering a package view from its rows.
//!
//! An npm packument is assembled from cached per-version fragments and the
//! mutable package state, and the assembled view is cached at the package's
//! generation. Every read checks that generation inside its own read
//! transaction, so a cached view is served only while it is current: a lost
//! invalidation cannot pin an old view.

use super::{
    DatabaseState, PackageKey,
    cache::{FragmentKey, RENDERER_VERSION, RenderCache, Representation, ViewKey},
    commit::{PackageRow, package_row, parse_json, parse_optional_json},
    packument::{
        DEPRECATED, ORDINARY, README, README_FILENAME, SUBMITTED_PROFILE, TAGS, TIME, VERSIONS,
    },
    tags::stored_tags,
    to_generation,
};
use crate::metadata::PackageView;
use pnpr_error::Result;
use pnpr_package_name::Ecosystem;
use rusqlite::{Connection, params};
use serde_json::{Map, Value};
use std::{collections::BTreeMap, sync::Arc};

pub(super) fn read_view(
    state: &mut DatabaseState,
    key: &PackageKey,
) -> Result<Option<PackageView>> {
    let DatabaseState { connection, cache } = state;
    let snapshot = connection.transaction()?;
    let Some(package) = package_row(&snapshot, key)? else { return Ok(None) };
    let view_key = ViewKey { package: key.clone(), representation: Representation::Full };
    let bytes = if let Some(bytes) = cache.view(&view_key, package.generation) {
        bytes
    } else {
        let bytes: Arc<[u8]> = match key.ecosystem {
            Ecosystem::Npm => assemble_packument(&snapshot, cache, key, &package)?.into(),
            Ecosystem::Cargo | Ecosystem::Pypi | Ecosystem::Oci => package.state.into(),
        };
        cache.store_view(view_key, package.generation, Arc::clone(&bytes));
        bytes
    };
    Ok(Some(PackageView { bytes, generation: Some(key.generation(package.generation)) }))
}

/// One version as the version list reads it, before its fragment.
struct VersionEntry {
    version: String,
    published_at: Option<String>,
    fragment: FragmentKey,
}

fn version_entries(connection: &Connection, key: &PackageKey) -> Result<Vec<VersionEntry>> {
    let mut statement = connection.prepare_cached(
        "SELECT v.version, v.published_at, v.accepted_generation, a.generation
         FROM versions v
         LEFT JOIN annotations a USING (catalog, ecosystem, name, version)
         WHERE v.catalog = ?1 AND v.ecosystem = ?2 AND v.name = ?3 AND v.revision_key = ?4
         ORDER BY v.version",
    )?;
    let (catalog, ecosystem, name) = key.params();
    let entries = statement.query_map(params![catalog, ecosystem, name, ORDINARY], |row| {
        let version: String = row.get(0)?;
        let fragment = FragmentKey {
            package: key.clone(),
            version: version.clone(),
            revision_key: ORDINARY.to_string(),
            accepted_generation: to_generation(row.get(2)?),
            profile: SUBMITTED_PROFILE,
            representation: Representation::Full,
            renderer: RENDERER_VERSION,
            annotation_generation: row.get::<_, Option<i64>>(3)?.map(to_generation),
        };
        Ok(VersionEntry { version, published_at: row.get(1)?, fragment })
    })?;
    Ok(entries.collect::<rusqlite::Result<_>>()?)
}

/// Render one version's manifest with its README and deprecation.
fn render_fragment(connection: &Connection, key: &FragmentKey) -> Result<Arc<str>> {
    let [manifest, fields @ ..] = fragment_columns(connection, key)?;
    let mut fragment: Map<String, Value> = parse_json(manifest.as_deref().unwrap_or("{}"))?;
    for (field, value) in [README, README_FILENAME, DEPRECATED].into_iter().zip(fields) {
        if let Some(value) = parse_optional_json(value.as_deref())? {
            fragment.insert(field.to_string(), value);
        }
    }
    Ok(serde_json::to_string(&fragment)?.into())
}

/// The manifest, README, README filename, and deprecation of a version, as
/// stored.
fn fragment_columns(connection: &Connection, key: &FragmentKey) -> Result<[Option<String>; 4]> {
    let (catalog, ecosystem, name) = key.package.params();
    let columns = connection
        .prepare_cached(
            "SELECT m.manifest, r.readme, r.readme_filename, a.deprecated
             FROM accepted_manifests m
             LEFT JOIN readmes r USING (catalog, ecosystem, name, version, revision_key)
             LEFT JOIN annotations a USING (catalog, ecosystem, name, version)
             WHERE m.catalog = ?1 AND m.ecosystem = ?2 AND m.name = ?3 AND m.version = ?4
               AND m.revision_key = ?5 AND m.profile = ?6",
        )?
        .query_row(
            params![catalog, ecosystem, name, key.version, key.revision_key, key.profile],
            |row| Ok([row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?]),
        )?;
    Ok(columns)
}

fn fragment(
    connection: &Connection,
    cache: &mut RenderCache,
    key: &FragmentKey,
) -> Result<Arc<str>> {
    if let Some(fragment) = cache.fragment(key) {
        return Ok(fragment);
    }
    let fragment = render_fragment(connection, key)?;
    cache.store_fragment(key.clone(), Arc::clone(&fragment));
    Ok(fragment)
}

/// The parts of a packument that are not package state.
struct Assembled {
    tags: BTreeMap<String, String>,
    versions: Vec<(String, Arc<str>)>,
    /// `time` entries of the versions, in version order.
    published: Vec<(String, String)>,
}

fn assemble_packument(
    connection: &Connection,
    cache: &mut RenderCache,
    key: &PackageKey,
    package: &PackageRow,
) -> Result<Vec<u8>> {
    let state: Map<String, Value> = serde_json::from_slice(&package.state)?;
    let mut assembled = Assembled {
        tags: stored_tags(connection, key)?,
        versions: Vec::new(),
        published: Vec::new(),
    };
    for entry in version_entries(connection, key)? {
        let fragment = fragment(connection, cache, &entry.fragment)?;
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
