use super::{
    HostedGit, LicenseDetails, extract_license, is_unsafe_path_component, license_resolver,
    safe_read_package_json_from_dir,
};

/// The package's declared license, falling back to a license file in its
/// directory when the manifest declares none or defers to one.
pub(super) async fn read_license_details(pkg_dir: &std::path::Path, name: &str) -> LicenseDetails {
    let manifest = if is_unsafe_path_component(name) {
        None
    } else {
        safe_read_package_json_from_dir(pkg_dir).unwrap_or(None)
    };
    let Some(manifest) = manifest else {
        return LicenseDetails {
            license: "Unknown".to_string(),
            author: None,
            homepage: None,
            description: None,
        };
    };
    let license = match extract_license(&manifest) {
        Some(license) if !license.to_ascii_lowercase().contains("see license") => license,
        manifest_license => license_resolver::resolve_license_from_dir(manifest_license, pkg_dir)
            .await
            .unwrap_or_else(|| "Unknown".to_string()),
    };
    LicenseDetails {
        license,
        author: extract_license_author(&manifest),
        homepage: extract_license_homepage(&manifest),
        description: manifest
            .get("description")
            .and_then(serde_json::Value::as_str)
            .map(ToString::to_string),
    }
}

pub(super) fn extract_license_author(manifest: &serde_json::Value) -> Option<String> {
    match manifest.get("author")? {
        serde_json::Value::String(author) => {
            if author.is_empty() {
                return Some(String::new());
            }
            let name_end = author
                .find(['(', '<'])
                .unwrap_or(author.len());
            let name = author[..name_end].trim();
            (!name.is_empty()).then(|| name.to_string())
        }
        serde_json::Value::Object(author) => author
            .get("name")
            .and_then(serde_json::Value::as_str)
            .map(ToString::to_string),
        _ => None,
    }
}

pub(super) fn extract_license_homepage(manifest: &serde_json::Value) -> Option<String> {
    if let Some(homepage) = manifest
        .get("homepage")
        .and_then(serde_json::Value::as_str)
        .filter(|url| !url.is_empty())
    {
        return Some(if url::Url::parse(homepage).is_ok() {
            homepage.to_string()
        } else {
            format!("http://{homepage}")
        });
    }

    let repository = match manifest.get("repository")? {
        serde_json::Value::String(repository) => repository,
        serde_json::Value::Object(repository) => {
            repository.get("url").and_then(serde_json::Value::as_str)?
        }
        _ => return None,
    };
    HostedGit::package_docs_url(repository)
}
