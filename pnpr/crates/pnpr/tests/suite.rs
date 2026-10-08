// Shared helpers, declared once so the files below can use them without
// each loading a copy.
#[path = "common/ecosystem.rs"]
mod ecosystem;
#[path = "common/network.rs"]
mod network;
#[path = "common/npm.rs"]
mod npm;
#[path = "common/pausing_store.rs"]
mod pausing_store;
#[path = "common/registry_groups.rs"]
mod registry_groups;
#[path = "common/storage.rs"]
mod storage;
#[path = "common/tokens.rs"]
mod tokens;

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
mod rule_administration;
mod s3_backend;
mod scim;
mod server;
mod staged_publish;
mod team_administration;
mod team_package_access;
mod user_administration;
