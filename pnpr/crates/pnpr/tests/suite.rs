// Shared helpers, declared once so the files below can use them without
// each loading a copy.
#[path = "common/ecosystem.rs"]
mod ecosystem;
#[path = "common/npm.rs"]
mod npm;
#[path = "common/pausing_store.rs"]
mod pausing_store;
#[path = "common/registry_groups.rs"]
mod registry_groups;
#[path = "common/storage.rs"]
mod storage;

mod auth_persistence;
mod auth_publish;
mod auth_user_endpoints;
mod batch_publish;
mod cargo_registry;
mod cargo_resolve;
mod cross_ecosystem_publish;
mod journal_recovery;
mod multi_replica;
mod oci_registry;
mod policy;
mod pypi_registry;
mod pypi_resolve;
mod registry_mock;
mod s3_backend;
mod server;
mod staged_publish;
