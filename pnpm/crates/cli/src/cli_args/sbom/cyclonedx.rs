use super::{
    DepType, HashMap, SbomComponent, SbomComponentType, SbomResult, base64_to_hex, build_purl,
    classify_license, generate_uuid_v4,
};

pub(super) struct CycloneDxOpts<'a> {
    pub(super) result: &'a SbomResult,
    pub(super) spec_version: Option<&'a str>,
    pub(super) lockfile_only: bool,
    pub(super) authors: &'a [String],
    pub(super) supplier: Option<&'a str>,
    pub(super) compact: bool,
}

pub(super) fn split_scoped_name(name: &str) -> (Option<&str>, &str) {
    if name.starts_with('@') {
        if let Some(idx) = name.find('/') {
            (Some(&name[..idx]), &name[idx + 1..])
        } else {
            (None, name)
        }
    } else {
        (None, name)
    }
}

pub(super) fn serialize_cyclonedx(opts: &CycloneDxOpts<'_>) -> String {
    let result = opts.result;
    let spec_version = opts.spec_version.unwrap_or("1.7");
    let root_type = match result.root_type {
        SbomComponentType::Library => "library",
        SbomComponentType::Application => "application",
    };

    let root_purl = build_purl(&result.root_name, &result.root_version);
    let root_component = cyclonedx_root_component(result, &root_purl, root_type);
    let components: Vec<serde_json::Value> =
        result.components.iter().map(cyclonedx_component).collect();
    let dependencies = cyclonedx_dependencies(result, &root_purl);

    let metadata = cyclonedx_metadata(opts, &root_component);

    let bom = serde_json::json!({
        "$schema": format!("http://cyclonedx.org/schema/bom-{spec_version}.schema.json"),
        "bomFormat": "CycloneDX",
        "specVersion": spec_version,
        "serialNumber": format!("urn:uuid:{}", generate_uuid_v4()),
        "version": 1,
        "metadata": metadata,
        "components": components,
        "dependencies": dependencies,
    });

    if opts.compact {
        serde_json::to_string(&bom).expect("JSON serialization")
    } else {
        serde_json::to_string_pretty(&bom).expect("JSON serialization")
    }
}

fn cyclonedx_metadata(
    opts: &CycloneDxOpts<'_>,
    root_component: &serde_json::Value,
) -> serde_json::Value {
    let timestamp = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let phase = if opts.lockfile_only { "pre-build" } else { "build" };

    let mut metadata = serde_json::json!({
        "timestamp": timestamp,
        "lifecycles": [{ "phase": phase }],
        "tools": { "components": [{
            "type": "application",
            "name": "pnpm",
            "version": pnpm_config::PNPM_VERSION,
        }] },
        "component": root_component,
    });

    if !opts.authors.is_empty() {
        let author_list: Vec<serde_json::Value> =
            opts.authors.iter().map(|name| serde_json::json!({ "name": name })).collect();
        metadata["authors"] = serde_json::Value::Array(author_list);
    }
    if let Some(supplier) = opts.supplier {
        metadata["supplier"] = serde_json::json!({ "name": supplier });
    }
    metadata
}

/// The `metadata.component` describing the project the SBOM is for.
fn cyclonedx_root_component(
    result: &SbomResult,
    root_purl: &str,
    root_type: &str,
) -> serde_json::Value {
    let (root_group, root_name) = split_scoped_name(&result.root_name);
    let mut root_component = serde_json::json!({
        "type": root_type,
        "name": root_name,
        "version": result.root_version,
        "purl": root_purl,
        "bom-ref": root_purl,
    });
    if let Some(group) = root_group {
        root_component["group"] = serde_json::Value::String(group.to_string());
    }
    if let Some(description) = &result.root_description {
        root_component["description"] = serde_json::Value::String(description.clone());
    }
    if let Some(author) = &result.root_author {
        root_component["authors"] = serde_json::json!([{ "name": author }]);
    }
    if let Some(license) = &result.root_license {
        root_component["licenses"] = serde_json::json!([classify_license(license)]);
    }
    let mut root_ext_refs: Vec<serde_json::Value> = Vec::new();
    if let Some(repository) = &result.root_repository {
        root_ext_refs.push(serde_json::json!({ "type": "vcs", "url": repository }));
    }
    if let Some(bugs) = &result.root_bugs_url {
        root_ext_refs.push(serde_json::json!({ "type": "issue-tracker", "url": bugs }));
    }
    if !root_ext_refs.is_empty() {
        root_component["externalReferences"] = serde_json::Value::Array(root_ext_refs);
    }
    root_component
}

/// One installed package as a `CycloneDX` component.
fn cyclonedx_component(component: &SbomComponent) -> serde_json::Value {
    let (group, name) = split_scoped_name(&component.name);
    let mut comp = serde_json::json!({
        "type": "library",
        "name": name,
        "version": component.version,
        "purl": component.purl,
        "bom-ref": component.purl,
    });
    if let Some(group) = group {
        comp["group"] = serde_json::Value::String(group.to_string());
    }
    if component.dep_type == DepType::DevOnly {
        comp["scope"] = serde_json::Value::String("excluded".to_string());
        comp["properties"] =
            serde_json::json!([{ "name": "cdx:npm:package:development", "value": "true" }]);
    }
    if let Some(description) = &component.description {
        comp["description"] = serde_json::Value::String(description.clone());
    }
    if let Some(author) = &component.author {
        comp["authors"] = serde_json::json!([{ "name": author }]);
    }
    if let Some(license) = &component.license {
        comp["licenses"] = serde_json::json!([classify_license(license)]);
    }
    let ext_refs = cyclonedx_external_references(component);
    if !ext_refs.is_empty() {
        comp["externalReferences"] = serde_json::Value::Array(ext_refs);
    }
    comp
}

/// Where a component came from: its archive (with the integrity the
/// lockfile pins), its homepage, its repository and its issue tracker.
fn cyclonedx_external_references(component: &SbomComponent) -> Vec<serde_json::Value> {
    let mut ext_refs: Vec<serde_json::Value> = Vec::new();
    if let Some(tarball) = &component.tarball_url {
        let mut dist_ref = serde_json::json!({ "type": "distribution", "url": tarball });
        if let Some(integrity) = &component.integrity
            && let Some(hashes) = integrity_to_hashes(integrity)
        {
            dist_ref["hashes"] = serde_json::Value::Array(hashes);
        }
        ext_refs.push(dist_ref);
    }
    if let Some(homepage) = &component.homepage {
        ext_refs.push(serde_json::json!({ "type": "website", "url": homepage }));
    }
    if let Some(repository) = &component.repository {
        ext_refs.push(serde_json::json!({ "type": "vcs", "url": repository }));
    }
    if let Some(bugs) = &component.bugs_url {
        ext_refs.push(serde_json::json!({ "type": "issue-tracker", "url": bugs }));
    }
    ext_refs
}

/// The `dependencies` graph, with one entry per component — including
/// the ones nothing depends on — in a deterministic order.
fn cyclonedx_dependencies(result: &SbomResult, root_purl: &str) -> Vec<serde_json::Value> {
    let mut deps_map: HashMap<&str, Vec<&str>> = HashMap::new();
    deps_map.entry(root_purl).or_default();
    for component in &result.components {
        deps_map.entry(&component.purl).or_default();
    }
    for relationship in &result.relationships {
        deps_map.entry(&relationship.from).or_default().push(&relationship.to);
    }
    let mut refs: Vec<&&str> = deps_map.keys().collect();
    refs.sort_unstable();
    refs.iter()
        .map(|ref_purl| {
            let mut dep_list = deps_map[*ref_purl].clone();
            dep_list.sort_unstable();
            dep_list.dedup();
            serde_json::json!({ "ref": ref_purl, "dependsOn": dep_list })
        })
        .collect()
}

fn integrity_to_hashes(integrity: &str) -> Option<Vec<serde_json::Value>> {
    let mut hashes = Vec::new();
    for part in integrity.split_whitespace() {
        let Some((alg, hash)) = part.split_once('-') else { continue };
        let cdx_alg = match alg {
            "sha1" => "SHA-1",
            "sha256" => "SHA-256",
            "sha384" => "SHA-384",
            "sha512" => "SHA-512",
            "md5" => "MD5",
            _ => continue,
        };
        let hex = base64_to_hex(hash)?;
        hashes.push(serde_json::json!({
            "alg": cdx_alg,
            "content": hex,
        }));
    }
    if hashes.is_empty() { None } else { Some(hashes) }
}
