use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

pub const CAS_MANIFEST_FILENAME: &str = ".pnpm-store.json";
pub const CAS_LOADER_FILENAME: &str = ".pnpm-store-loader.mjs";

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreLoaderManifest {
    pub version: u8,
    pub store_dir: PathBuf,
    pub packages: BTreeMap<String, StoreLoaderPackage>,
}

#[derive(Serialize, Deserialize)]
pub struct StoreLoaderPackage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution: Option<String>,
    #[serde(default)]
    pub dependencies: BTreeMap<String, String>,
}
