use super::{HoistKind, Modules};
use std::iter;

/// Translate the legacy `shamefullyHoist` and `hoistedAliases` fields into
/// the modern `publicHoistPattern` and `hoistedDependencies` shapes.
pub(super) fn apply_legacy_shamefully_hoist(manifest: &mut Modules) {
    let Some(shamefully_hoist) = manifest.shamefully_hoist else {
        return;
    };
    let kind = if shamefully_hoist {
        HoistKind::Public
    } else {
        HoistKind::Private
    };
    match (&manifest.public_hoist_pattern, shamefully_hoist) {
        (None, false) => manifest.public_hoist_pattern = Some(Vec::new()),
        (None, true) => manifest.public_hoist_pattern = Some(vec!["*".to_string()]),
        (Some(_), _) => {}
    }
    if manifest.hoisted_dependencies.is_empty()
        && let Some(aliases_by_path) = &manifest.hoisted_aliases
    {
        manifest.hoisted_dependencies = aliases_by_path
            .iter()
            .map(|(dep_path, alias_names)| {
                let entry = alias_names
                    .iter()
                    .cloned()
                    .zip(iter::repeat(kind))
                    .collect();
                (dep_path.clone().into(), entry)
            })
            .collect();
    }
}

/// Drop the legacy `hoistedAliases` field on write when neither hoist
/// pattern is present.
pub(super) fn drop_legacy_hoisted_aliases_when_unreferenced(manifest: &mut Modules) {
    if manifest.hoist_pattern.is_none() && manifest.public_hoist_pattern.is_none() {
        manifest.hoisted_aliases = None;
    }
}
