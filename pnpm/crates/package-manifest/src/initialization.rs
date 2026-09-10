use super::{PackageManifest, PackageManifestError, Path, Value, json};

/// What `pnpm init` records in the manifest it scaffolds, beyond the fields
/// every scaffold carries.
#[derive(Debug, Default, Clone, Copy)]
pub struct InitOptions<'a> {
    /// Record the package as an ES module (`"type": "module"`). The
    /// `commonjs` alternative leaves the field out, since that is what Node
    /// assumes when it is absent. The `initType` setting.
    pub es_module: bool,

    /// The pnpm version to record as the project's package-manager pin, or
    /// `None` to leave the project unpinned. See [`PackageManifest::init`].
    pub pinned_pnpm_version: Option<&'a str>,

    /// Who to credit in the `author` field. An [`InitAuthor`] with no part
    /// set renders empty, and the scaffold's empty `author` stands.
    pub author: InitAuthor<'a>,

    /// The `license` the scaffold records instead of its `ISC` default.
    /// The `initLicense` setting.
    pub license: Option<&'a str>,

    /// The `version` the scaffold records instead of its `1.0.0` default.
    /// The `initVersion` setting.
    pub version: Option<&'a str>,
}

/// The `initAuthorName` / `initAuthorEmail` / `initAuthorUrl` settings,
/// which together make up the `author` field `pnpm init` writes.
#[derive(Debug, Default, Clone, Copy)]
pub struct InitAuthor<'a> {
    pub name: Option<&'a str>,
    pub email: Option<&'a str>,
    pub url: Option<&'a str>,
}

impl PackageManifest {
    pub(super) fn create_init_package_json(name: &str, options: InitOptions<'_>) -> Value {
        let mut manifest = json!({
            "name": name,
            "version": "1.0.0",
            "description": "",
            "main": "index.js",
            "scripts": {
              "test": r#"echo "Error: no test specified" && exit 1"#
            },
            "keywords": [],
            "author": "",
            "license": "ISC"
        });
        let fields = manifest.as_object_mut().expect("the scaffold is a JSON object");
        if let Some(version) = options.pinned_pnpm_version {
            // The pin is written twice on purpose: pnpm reads
            // `devEngines.packageManager`, corepack reads only the legacy
            // `packageManager` field. The two must agree — pnpm warns and
            // ignores the legacy field when they disagree — and corepack
            // rejects everything but an exact version, so neither carries a
            // range.
            fields.insert(
                "devEngines".to_string(),
                json!({
                    "packageManager": {
                        "name": "pnpm",
                        "version": version,
                        "onFail": "download",
                    },
                }),
            );
            fields.insert("packageManager".to_string(), json!(format!("pnpm@{version}")));
        }
        if options.es_module {
            fields.insert("type".to_string(), json!("module"));
        }
        // The `init*` settings overwrite the scaffold's placeholders in
        // place, so the key order the scaffold fixes is preserved.
        if let Some(version) = options.version {
            fields.insert("version".to_string(), json!(version));
        }
        if let Some(license) = options.license {
            fields.insert("license".to_string(), json!(license));
        }
        let author = options.author.to_string();
        if !author.is_empty() {
            fields.insert("author".to_string(), json!(author));
        }
        manifest
    }

    /// The scaffold manifest `pnpm init` (and [`Self::create_if_needed`])
    /// produces for `path`, named after the containing directory.
    ///
    /// See [`InitOptions`] for the fields `options` controls, and
    /// [`Self::init`] for when a package-manager pin is written.
    #[must_use]
    pub fn init_value_for(path: &Path, options: InitOptions<'_>) -> Value {
        let name = path
            .parent()
            .and_then(|folder| folder.file_name())
            .and_then(|file_name| file_name.to_str())
            .unwrap_or("");
        PackageManifest::create_init_package_json(name, options)
    }

    /// Scaffold a `package.json` at `path`, failing if one is already there.
    ///
    /// [`InitOptions::pinned_pnpm_version`] pins the project to that pnpm
    /// version; the caller passes `None` when the `initPackageManager`
    /// setting is off, or when `path` is a member of an existing workspace
    /// and therefore inherits the root's pin.
    pub fn init(path: &Path, options: InitOptions<'_>) -> Result<(), PackageManifestError> {
        if path.exists() {
            return Err(PackageManifestError::AlreadyExist);
        }
        let manifest = PackageManifest::init_value_for(path, options);
        let contents = PackageManifest::write_to_file(path, &manifest)?;
        println!("Wrote to {path}\n\n{contents}", path = path.display());
        Ok(())
    }
}
