use crate::State;
use clap::Args;
use miette::{Context, IntoDiagnostic, Result};
use pacquet_resolving_npm_resolver::{
    InMemoryPackageMetaCache, PickPackageContext, PickPackageOptions, parse_bare_specifier,
    pick_package, pick_registry_for_package, shared_packument_fetch_locker,
};
use pacquet_resolving_parse_wanted_dependency::parse_wanted_dependency;
use pacquet_store_dir::{StoreIndex, store_index_key};
use serde_json::Value;

#[derive(Debug, Args)]
pub struct CatIndexArgs {
    /// The package specifier (e.g., `pkg@version`)
    pub wanted_dependency: String,
}

impl CatIndexArgs {
    pub async fn run(self, state: State) -> Result<()> {
        let parsed = parse_wanted_dependency(&self.wanted_dependency);
        let Some(alias) = parsed.alias else {
            return Err(miette::miette!(
                "Cannot parse the \"{}\" selector",
                self.wanted_dependency
            ));
        };

        let bare_specifier = parsed.bare_specifier.unwrap_or_else(|| "latest".to_string());

        let config = state.config;
        let registries: std::collections::HashMap<String, String> =
            config.resolved_registries().into_iter().collect();
        let registry = pick_registry_for_package(&registries, &alias, None);

        let Some(spec_parsed) =
            parse_bare_specifier(&bare_specifier, Some(&alias), "latest", &registry)
        else {
            return Err(miette::miette!("Invalid specifier for registry resolution"));
        };

        let meta_cache = InMemoryPackageMetaCache::default();
        let fetch_locker = shared_packument_fetch_locker();

        let ctx = PickPackageContext {
            http_client: &state.http_client,
            auth_headers: &config.auth_headers,
            meta_cache: &meta_cache,
            fetch_locker: &fetch_locker,
            cache_dir: Some(&config.cache_dir),
            offline: config.offline,
            prefer_offline: config.prefer_offline,
            ignore_missing_time_field: config.minimum_release_age_ignore_missing_time,
            full_metadata: false,
            filter_metadata: false,
            retry_opts: pacquet_network::RetryOpts::default(),
        };

        let opts = PickPackageOptions {
            registry: &registry,
            preferred_version_selectors: None,
            published_by: None,
            published_by_exclude: None,
            pick_lowest_version: false,
            include_latest_tag: false,
            dry_run: false,
            optional: false,
            update_checksums: false,
            blocked_versions: None,
        };

        let pick = pick_package(&ctx, &spec_parsed, &opts)
            .await
            .into_diagnostic()
            .wrap_err("Failed to resolve package")?;

        let Some(picked) = pick.picked_package else {
            return Err(miette::miette!("No version found matching the specifier"));
        };

        let integrity = picked
            .dist
            .integrity
            .clone()
            .unwrap_or_else(|| panic!("Package resolved without integrity"));

        let files_index_file =
            store_index_key(&integrity.to_string(), &format!("{}@{}", alias, picked.version));

        let store_dir = config.store_dir.root();

        if !store_dir.join("index.db").exists() {
            return Err(miette::miette!(
                "No corresponding index file found. You can use pnpm list to see if the package is installed."
            ));
        }

        let store_index = StoreIndex::open_readonly(store_dir).into_diagnostic()?;

        let pkg_files_index = store_index.get(&files_index_file).into_diagnostic()?;

        let Some(pkg_files_index) = pkg_files_index else {
            return Err(miette::miette!(
                "No corresponding index file found. You can use pnpm list to see if the package is installed."
            ));
        };

        // pnpm's sortDeepKeys equivalent
        let mut value = serde_json::to_value(&pkg_files_index).into_diagnostic()?;
        sort_deep_keys(&mut value);

        let json = serde_json::to_string_pretty(&value).into_diagnostic()?;
        println!("{json}");

        Ok(())
    }
}

fn sort_deep_keys(value: &mut Value) {
    if let Value::Object(map) = value {
        let mut sorted = serde_json::Map::new();
        let mut keys: Vec<_> = map.keys().cloned().collect();
        keys.sort(); // Simple string comparison

        for key in keys {
            let mut val = map.remove(&key).unwrap();
            sort_deep_keys(&mut val);
            sorted.insert(key, val);
        }
        *map = sorted;
    } else if let Value::Array(arr) = value {
        for item in arr {
            sort_deep_keys(item);
        }
    }
}
