//! Native workspace release management: the engine behind `pnpm change` and
//! the bare `pnpm version -r`. Reads and writes changesets-compatible change
//! intents from `.changeset/*.md`, assembles a release plan (direct bumps,
//! dependent propagation through materialized `workspace:` ranges, fixed
//! groups, per-package release lanes), and applies it (manifest version
//! updates, changelog composition, the consumed-intents ledger, and
//! intent-file cleanup).
//!
//! The TypeScript counterpart is `@pnpm/releasing.versioning`
//! (`pnpm11/releasing/versioning`); the two must stay behaviorally identical.
//! See the native monorepo versioning RFC:
//! <https://github.com/pnpm/rfcs/pull/18>.

mod apply;
mod changelog;
mod error;
mod human_id;
mod intents;
mod ledger;
mod pending;
mod plan;
mod settings;

pub use apply::{AppliedRelease, apply_release_plan};
pub use changelog::{compose_changelog_section, prepend_changelog_section, render_changelog};
pub use error::VersioningError;
pub use intents::{
    CHANGES_DIR, ChangeIntent, IntentBumpType, parse_change_intent, read_change_intents,
    write_change_intent,
};
pub use ledger::{
    LEDGER_FILENAME, Ledger, LedgerEntry, PackageConsumption, append_to_ledger,
    build_consumption_index, normalize_project_dir, read_ledger,
};
pub use pending::{
    PENDING_CHANGELOGS_DIR, pending_changelog_path, read_pending_changelog,
    remove_pending_changelog, write_pending_changelog,
};
pub use plan::{
    AssembleReleasePlanOptions, DependencyField, DependencyUpdate, ManifestDependency,
    PlannedRelease, ProjectRefIndex, ReleaseCause, ReleasePlan, WorkspaceProject,
    assemble_release_plan, index_project_refs, is_dir_ref, materialize_workspace_range,
    to_project_dir,
};
pub use settings::{
    ChangelogSettings, ChangelogStorage, ReleaseBumpType, VersioningSettings, changelog_storage,
};
