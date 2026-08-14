pub mod access;
pub mod add;
pub mod approve_builds;
pub mod audit;
pub mod bin;
pub mod bugs;
pub mod cache;
pub mod cat_file;
pub mod cat_index;
pub mod change;
pub mod changelog;
pub mod ci;
pub mod clean;
pub mod completion;
pub mod config;
pub(crate) mod config_warnings;
pub mod create;
pub mod dedupe;
pub mod deploy;
pub mod deprecate;
pub(crate) mod deps_tree;
pub mod dist_tag;
pub mod dlx;
pub mod docs;
pub mod doctor;
pub mod exec;
pub mod fetch;
pub mod find_hash;
pub mod global;
pub mod ignored_builds;
pub mod import;
pub mod install;
pub mod install_test;
pub mod lane;
pub(crate) mod legacy_pnpm_field;
pub mod licenses;
pub mod link;
pub mod list;
pub mod login;
pub mod logout;
pub mod outdated;
pub(crate) mod override_version_references;
pub mod owner;
pub mod pack;
pub mod pack_app;

pub mod patch;
pub mod patch_commit;
pub mod patch_remove;
pub(crate) mod patch_state;
pub mod peers;
pub mod ping;
pub mod pkg;
pub mod prefix;
pub mod prune;
pub mod publish;
pub mod rebuild;
pub mod recursive;
pub mod registry_client;
pub mod remove;
pub mod repo;
pub mod restart;
pub mod root;
pub mod run;
pub mod runtime;
pub mod sanitize;
pub mod sbom;
pub mod script_shortcut;
pub mod search;
pub mod self_update;
pub mod set_script;
pub mod setup;
pub mod stage;
pub mod star;
pub mod stars;
pub mod store;
pub mod sudo_guard;
pub mod supported_architectures;
pub mod team;
pub mod undeprecate;
pub mod unlink;
pub mod unpublish;
pub mod unstar;
pub mod update;
mod update_changeset;
pub mod update_interactive;
pub mod version;
pub mod view;
pub mod whoami;
pub mod why;
pub mod with;

pub(crate) mod cli_command;
mod dispatch;
mod dispatch_install;
mod dispatch_query;
mod dispatch_script;
mod package_manager;
mod pipelines;
pub(crate) mod pre_command;
pub(crate) mod reporter;
mod verify_deps;

pub(crate) use cli_command::CliArgs;

/// The CLI grammar, built once per process. Constructing it walks every
/// subcommand, and the passes that run before the parse each need the same
/// view of it — argument arity, aliases, and which positionals name a
/// subcommand.
pub(crate) fn grammar() -> &'static clap::Command {
    static GRAMMAR: std::sync::OnceLock<clap::Command> = std::sync::OnceLock::new();
    GRAMMAR.get_or_init(<CliArgs as clap::CommandFactory>::command)
}

#[cfg(test)]
mod tests;
