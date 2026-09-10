use super::{
    HashMap, HashSet, SbomComponent, SbomComponentType, SbomResult, base64_to_hex, build_purl,
    generate_uuid_v4,
};

pub(super) fn sanitize_spdx_id(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' { ch } else { '-' })
        .collect()
}

pub(super) fn serialize_spdx(result: &SbomResult, compact: bool) -> String {
    let root_purl = build_purl(&result.root_name, &result.root_version);
    let root_spdx_id = "SPDXRef-RootPackage";
    let root_purpose = match result.root_type {
        SbomComponentType::Library => "LIBRARY",
        SbomComponentType::Application => "APPLICATION",
    };

    let mut spdx_id_map: HashMap<&str, String> = HashMap::new();
    spdx_id_map.insert(&root_purl, root_spdx_id.to_string());
    let mut spdx_packages = vec![spdx_root_package(result, &root_purl, root_spdx_id, root_purpose)];
    for (index, component) in result.components.iter().enumerate() {
        let spdx_id = format!(
            "SPDXRef-Package-{}-{}-{index}",
            sanitize_spdx_id(&component.name),
            sanitize_spdx_id(&component.version),
        );
        spdx_id_map.insert(&component.purl, spdx_id.clone());
        spdx_packages.push(spdx_component_package(component, &spdx_id));
    }
    let spdx_relationships = spdx_relationships(result, root_spdx_id, &spdx_id_map);

    let timestamp = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let doc_namespace = spdx_document_namespace(result);

    let doc = serde_json::json!({
        "spdxVersion": "SPDX-2.3",
        "dataLicense": "CC0-1.0",
        "SPDXID": "SPDXRef-DOCUMENT",
        "name": result.root_name,
        "documentNamespace": doc_namespace,
        "creationInfo": {
            "created": timestamp,
            "creators": ["Tool: pnpm"],
        },
        "packages": spdx_packages,
        "relationships": spdx_relationships,
    });

    if compact {
        serde_json::to_string(&doc).expect("JSON serialization")
    } else {
        serde_json::to_string_pretty(&doc).expect("JSON serialization")
    }
}

/// The document's relationships: it describes the root package, and each
/// dependency edge between packages it lists, deduplicated.
fn spdx_relationships(
    result: &SbomResult,
    root_spdx_id: &str,
    spdx_id_map: &HashMap<&str, String>,
) -> Vec<serde_json::Value> {
    let mut relationships: Vec<serde_json::Value> = vec![serde_json::json!({
        "spdxElementId": "SPDXRef-DOCUMENT",
        "relatedSpdxElement": root_spdx_id,
        "relationshipType": "DESCRIBES",
    })];
    let mut seen_rels: HashSet<(&str, &str)> = HashSet::new();
    for relationship in &result.relationships {
        let (Some(from_id), Some(to_id)) = (
            spdx_id_map.get(relationship.from.as_str()),
            spdx_id_map.get(relationship.to.as_str()),
        ) else {
            continue;
        };
        if !seen_rels.insert((from_id.as_str(), to_id.as_str())) {
            continue;
        }
        relationships.push(serde_json::json!({
            "spdxElementId": from_id,
            "relatedSpdxElement": to_id,
            "relationshipType": "DEPENDS_ON",
        }));
    }
    relationships
}

/// One installed package as an SPDX package.
fn spdx_component_package(component: &SbomComponent, spdx_id: &str) -> serde_json::Value {
    let comp_license = component.license.as_deref().unwrap_or("NOASSERTION");
    let download_loc = component.tarball_url.as_deref().unwrap_or("NOASSERTION");
    let mut pkg = serde_json::json!({
        "SPDXID": spdx_id,
        "name": component.name,
        "versionInfo": component.version,
        "downloadLocation": download_loc,
        "filesAnalyzed": false,
        "licenseConcluded": comp_license,
        "licenseDeclared": comp_license,
        "copyrightText": "NOASSERTION",
        "externalRefs": [{
            "referenceCategory": "PACKAGE-MANAGER",
            "referenceType": "purl",
            "referenceLocator": component.purl,
        }],
    });
    if let Some(description) = &component.description {
        pkg["description"] = serde_json::Value::String(description.clone());
    }
    if let Some(homepage) = &component.homepage {
        pkg["homepage"] = serde_json::Value::String(homepage.clone());
    }
    if let Some(author) = &component.author {
        pkg["supplier"] = serde_json::Value::String(format!("Person: {author}"));
    }
    if let Some(integrity) = &component.integrity
        && let Some(checksums) = integrity_to_spdx_checksums(integrity)
    {
        pkg["checksums"] = serde_json::Value::Array(checksums);
    }
    pkg
}

/// The SPDX package describing the project the SBOM is for.
fn spdx_root_package(
    result: &SbomResult,
    root_purl: &str,
    root_spdx_id: &str,
    root_purpose: &str,
) -> serde_json::Value {
    let license_value = result.root_license.as_deref().unwrap_or("NOASSERTION");
    let mut root_package = serde_json::json!({
        "SPDXID": root_spdx_id,
        "name": result.root_name,
        "versionInfo": result.root_version,
        "downloadLocation": "NOASSERTION",
        "filesAnalyzed": false,
        "primaryPackagePurpose": root_purpose,
        "licenseConcluded": license_value,
        "licenseDeclared": license_value,
        "copyrightText": "NOASSERTION",
        "externalRefs": [{
            "referenceCategory": "PACKAGE-MANAGER",
            "referenceType": "purl",
            "referenceLocator": root_purl,
        }],
    });
    if let Some(description) = &result.root_description {
        root_package["description"] = serde_json::Value::String(description.clone());
    }
    if let Some(author) = &result.root_author {
        root_package["supplier"] = serde_json::Value::String(format!("Person: {author}"));
    }
    if let Some(repository) = &result.root_repository {
        root_package["homepage"] = serde_json::Value::String(repository.clone());
    }
    root_package
}

fn integrity_to_spdx_checksums(integrity: &str) -> Option<Vec<serde_json::Value>> {
    let mut checksums = Vec::new();
    for part in integrity.split_whitespace() {
        let Some((alg, hash)) = part.split_once('-') else { continue };
        let spdx_alg = match alg {
            "sha1" => "SHA1",
            "sha256" => "SHA256",
            "sha384" => "SHA384",
            "sha512" => "SHA512",
            "md5" => "MD5",
            _ => continue,
        };
        let hex = base64_to_hex(hash)?;
        checksums.push(serde_json::json!({
            "algorithm": spdx_alg,
            "checksumValue": hex,
        }));
    }
    if checksums.is_empty() { None } else { Some(checksums) }
}

fn spdx_document_namespace(result: &SbomResult) -> String {
    format!(
        "https://spdx.org/spdxdocs/{}-{}-{}",
        sanitize_spdx_id(&result.root_name),
        result.root_version,
        generate_uuid_v4(),
    )
}
