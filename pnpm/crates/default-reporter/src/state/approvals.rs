use std::collections::{BTreeMap, BTreeSet};

use pnpm_package_name::short_package_label;

use super::{BlockSlot, Output, ReporterState};

#[derive(Default)]
pub(super) struct PendingApprovals {
    pub(super) builds: BTreeSet<String>,
    pub(super) skills: BTreeSet<String>,
    pub(super) emitted: bool,
    pub(super) slot: BlockSlot,
}

impl ReporterState {
    pub(crate) fn flush_pending_approvals(&mut self) -> Output {
        if !self.options.append_only {
            return Output::None;
        }
        self.render_pending_approvals();
        self.display.finish(true)
    }
}

/// Each event adds to the lists rather than replacing them: one command can
/// run several installs, such as one per `pnpm add -g` group, and the notice
/// covers all of them.
impl PendingApprovals {
    pub(super) fn add_builds(&mut self, package_names: &[String]) {
        Self::add(&mut self.builds, &mut self.emitted, package_names);
    }

    pub(super) fn add_skills(&mut self, package_names: &[String]) {
        Self::add(&mut self.skills, &mut self.emitted, package_names);
    }

    fn add(pending: &mut BTreeSet<String>, emitted: &mut bool, package_names: &[String]) {
        for package in package_names {
            if pending.insert(package.clone()) {
                *emitted = false;
            }
        }
    }
}

pub(super) fn pending_approval_packages(
    pending_builds: &BTreeSet<String>,
    pending_skills: &BTreeSet<String>,
) -> BTreeMap<String, BTreeSet<&'static str>> {
    let mut packages = BTreeMap::<String, BTreeSet<&'static str>>::new();
    for package in pending_builds {
        packages
            .entry(short_package_label(package))
            .or_default()
            .insert("build");
    }
    for package in pending_skills {
        packages
            .entry(package.clone())
            .or_default()
            .insert("skills");
    }
    packages
}
