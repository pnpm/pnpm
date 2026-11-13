use pnpr_config::Config;
use pnpr_storage::Storage;

/// The configured hosted store, once its layout is one this pnpr reads.
pub(crate) async fn open_hosted_storage(config: &Config) -> miette::Result<Storage> {
    let storage = Storage::from_config(&config.storage)
        .map_err(|err| miette::miette!("failed to initialize storage: {err}"))?;
    storage
        .check_hosted_layout()
        .await
        .map_err(|err| miette::miette!("{err}"))?;
    Ok(storage)
}

pub(crate) fn configured_hosted_namespaces(config: &Config) -> Vec<String> {
    let mut namespaces = Vec::new();
    for hosted in config.routing.hosted.values() {
        if !namespaces.contains(&hosted.namespace) {
            namespaces.push(hosted.namespace.clone());
        }
    }
    if namespaces.is_empty() {
        namespaces.push(String::new());
    }
    namespaces
}
