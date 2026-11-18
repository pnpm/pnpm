use std::collections::{BTreeMap, BTreeSet};

use pnpm_package_name::short_package_label;

use super::BlockSlot;

#[derive(Default)]
pub(super) struct PendingApprovals {
    pub(super) builds: Vec<String>,
    pub(super) skills: Vec<String>,
    pub(super) emitted: bool,
    pub(super) slot: BlockSlot,
}

pub(super) fn pending_approval_packages(
    pending_builds: &[String],
    pending_skills: &[String],
) -> BTreeMap<String, BTreeSet<&'static str>> {
    let mut packages = BTreeMap::<String, BTreeSet<&'static str>>::new();
    for package in pending_builds {
        packages
            .entry(short_package_label(package))
            .or_default()
            .insert("build");
    }
    for package in pending_skills {
        let build_label = packages
            .keys()
            .find(|label| *label == package)
            .cloned();
        packages
            .entry(build_label.unwrap_or_else(|| package.clone()))
            .or_default()
            .insert("skills");
    }
    packages
}
