//! What `--fail-if-no-match` does with a `--filter` selection that came
//! back empty.

use super::{NoMatchingProjects, no_projects_matched_message, notice_workspace_dir};
use pnpm_config::Config;
use std::path::{Path, PathBuf};

/// pnpm's `--fail-if-no-match`: a selection that came back empty ends the
/// run with exit code 1 instead of letting the command operate on no
/// project at all.
pub struct UnmatchedFilters {
    workspace_dir: PathBuf,
    /// How many projects the selectors were matched against. A workspace
    /// holding none at all is reported differently from one whose projects
    /// the selectors all missed.
    projects: usize,
}

impl UnmatchedFilters {
    /// Count another ecosystem's projects among the ones the selectors were
    /// matched against, so a workspace holding only those is not reported
    /// as holding no project at all.
    #[must_use]
    pub fn counting(mut self, projects: usize) -> Self {
        self.projects += projects;
        self
    }

    /// End the run the way pnpm does: it prints the sentence to stdout and
    /// sets `process.exitCode = 1`, so the message is printed here and the
    /// returned error carries [`super::NO_MATCHING_PROJECTS_CODE`], which
    /// `is_reported_error` recognizes as already-printed.
    pub fn report(self) -> miette::Report {
        let message = if self.projects == 0 {
            format!(r#"No projects found in "{}""#, self.workspace_dir.display())
        } else {
            no_projects_matched_message(&self.workspace_dir)
        };
        println!("{message}");
        NoMatchingProjects { message }.into()
    }
}

/// The `--fail-if-no-match` failure this selection earns, if any.
pub(super) fn unmatched_filters(
    selected_count: usize,
    all_count: usize,
    config: &Config,
    prefix: &Path,
) -> Option<UnmatchedFilters> {
    if !config.fail_if_no_match || selected_count != 0 {
        return None;
    }
    Some(UnmatchedFilters {
        workspace_dir: notice_workspace_dir(config, prefix).to_path_buf(),
        projects: all_count,
    })
}
