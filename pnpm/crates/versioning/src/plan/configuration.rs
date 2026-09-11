use pnpm_matcher::WildcardMatcher;

use super::{
    BTreeMap, ChangeIntent, HashMap, HashSet, IntentBumpType, Participant, ProjectRefIndex,
    ResolvedEpic, VersioningError, VersioningSettings, bump_release_order, is_dir_ref,
    normalize_project_dir, resolve_config_ref,
};

pub(super) fn resolve_lanes(
    refs: &ProjectRefIndex,
    versioning: Option<&VersioningSettings>,
) -> Result<BTreeMap<String, String>, VersioningError> {
    let mut lanes_by_dir = BTreeMap::new();
    let Some(settings) = versioning else {
        return Ok(lanes_by_dir);
    };
    for (reference, lane) in &settings.lanes {
        if lane.eq_ignore_ascii_case("main") {
            return Err(VersioningError::InvalidLaneName {
                pkg_name: reference.clone(),
                lane: lane.clone(),
            });
        }
        for dir in resolve_config_ref(refs, reference, "versioning.lanes")? {
            lanes_by_dir.insert(dir, lane.clone());
        }
    }
    Ok(lanes_by_dir)
}

pub(super) fn resolve_fixed_groups(
    refs: &ProjectRefIndex,
    participants: &BTreeMap<String, Participant<'_>>,
    versioning: Option<&VersioningSettings>,
) -> Result<Vec<Vec<String>>, VersioningError> {
    let mut groups = Vec::new();
    for group in versioning.map(|settings| settings.fixed.as_slice()).unwrap_or_default() {
        groups.push(resolve_fixed_group(refs, participants, group)?);
    }
    Ok(groups)
}

fn resolve_fixed_group(
    refs: &ProjectRefIndex,
    participants: &BTreeMap<String, Participant<'_>>,
    group: &[String],
) -> Result<Vec<String>, VersioningError> {
    let mut dirs = Vec::new();
    for reference in group {
        for dir in resolve_config_ref(refs, reference, "versioning.fixed")? {
            if participants.contains_key(&dir) {
                dirs.push(dir);
            }
        }
    }
    Ok(dirs)
}

pub(super) fn validate_fixed_group_lanes(
    fixed_groups: &[Vec<String>],
    lanes_by_dir: &BTreeMap<String, String>,
    versioning: Option<&VersioningSettings>,
) -> Result<(), VersioningError> {
    for (index, group) in fixed_groups.iter().enumerate() {
        let tags: HashSet<Option<&String>> =
            group.iter().map(|dir| lanes_by_dir.get(dir)).collect();
        if tags.len() > 1 {
            let declared =
                versioning.map(|settings| settings.fixed[index].clone()).unwrap_or_default();
            return Err(VersioningError::ConflictingConfig { group: declared });
        }
    }
    Ok(())
}

/// Resolves each configured epic to its lead directory and the set of member
/// directories its selectors match. The lead — a single named package with a
/// semver version — is excluded from its own membership. Membership selectors
/// match name globs, `./`-prefixed directory globs, and `!`-prefixed negations.
pub(super) fn resolve_epics(
    refs: &ProjectRefIndex,
    participants: &BTreeMap<String, Participant<'_>>,
    versioning: Option<&VersioningSettings>,
) -> Result<Vec<ResolvedEpic>, VersioningError> {
    let mut epics = Vec::new();
    for epic in versioning.map(|settings| settings.epics.as_slice()).unwrap_or_default() {
        let lead_dir = resolve_config_ref(refs, &epic.lead, "versioning.epics lead")?
            .into_iter()
            .next()
            .filter(|dir| participants.contains_key(dir))
            .ok_or_else(|| VersioningError::EpicUnknownLead { lead: epic.lead.clone() })?;
        let selectors: Vec<EpicSelector> =
            epic.packages.iter().map(|selector| compile_epic_selector(selector)).collect();
        let mut member_dirs = HashSet::new();
        for participant in participants.values() {
            if participant.dir == lead_dir {
                continue;
            }
            if matches_epic_selectors(&selectors, &participant.dir, participant.name) {
                member_dirs.insert(participant.dir.clone());
            }
        }
        epics.push(ResolvedEpic { lead_ref: epic.lead.clone(), lead_dir, member_dirs });
    }
    Ok(epics)
}

struct EpicSelector {
    negated: bool,
    /// Whether the pattern matches a project's directory rather than its name.
    on_dir: bool,
    pattern: WildcardMatcher,
}

fn compile_epic_selector(selector: &str) -> EpicSelector {
    let (negated, body) = match selector.strip_prefix('!') {
        Some(rest) => (true, rest),
        None => (false, selector),
    };
    let on_dir = is_dir_ref(body);
    let pattern = if on_dir { normalize_project_dir(body) } else { body.to_string() };
    EpicSelector { negated, on_dir, pattern: WildcardMatcher::new(&pattern) }
}

/// Whether a project is an epic member under pnpm's order-dependent selector
/// rule: each matching selector overrides the previous verdict, so the last
/// one to match decides — a positive include or a `!` negation — mirroring
/// `@pnpm/config.matcher`, where a later include can re-include a package an
/// earlier negation excluded.
fn matches_epic_selectors(selectors: &[EpicSelector], dir: &str, name: &str) -> bool {
    let mut included = false;
    for selector in selectors {
        let input = if selector.on_dir { dir } else { name };
        if selector.pattern.matches(input) {
            included = !selector.negated;
        }
    }
    included
}

/// Rejects epic configurations that cannot be attributed unambiguously: a
/// package matched by two epics, and a fixed group that straddles an epic
/// boundary (a group must sit entirely inside or entirely outside an epic, so
/// its members never disagree on whether they are band-constrained).
pub(super) fn validate_epics(
    epics: &[ResolvedEpic],
    fixed_groups: &[Vec<String>],
) -> Result<(), VersioningError> {
    assert_epics_disjoint(epics)?;
    for epic in epics {
        for group in fixed_groups {
            assert_fixed_group_fits_epic(epic, group)?;
        }
    }
    Ok(())
}

/// A directory can belong to at most one epic: two leads would each rebase
/// it to a different band.
fn assert_epics_disjoint(epics: &[ResolvedEpic]) -> Result<(), VersioningError> {
    let mut epic_of_member: HashMap<&str, &str> = HashMap::new();
    for epic in epics {
        for member_dir in &epic.member_dirs {
            if let Some(other) = epic_of_member.get(member_dir.as_str())
                && *other != epic.lead_ref
            {
                return Err(VersioningError::EpicOverlap {
                    member_dir: member_dir.clone(),
                    first_lead: (*other).to_string(),
                    second_lead: epic.lead_ref.clone(),
                });
            }
            epic_of_member.insert(member_dir.as_str(), &epic.lead_ref);
        }
    }
    Ok(())
}

/// A fixed group that reaches into an epic has to lie inside it: its
/// outsiders would be dragged to the epic's band by the shared version.
fn assert_fixed_group_fits_epic(
    epic: &ResolvedEpic,
    group: &[String],
) -> Result<(), VersioningError> {
    if !group.iter().any(|dir| epic.member_dirs.contains(dir)) {
        return Ok(());
    }
    let outsiders: Vec<String> = group
        .iter()
        .filter(|dir| !epic.member_dirs.contains(*dir))
        .map(|dir| format!("./{dir}"))
        .collect();
    if outsiders.is_empty() {
        return Ok(());
    }
    Err(VersioningError::EpicFixedGroupConflict {
        lead: epic.lead_ref.clone(),
        outsiders: outsiders.join(", "),
    })
}

/// Resolves every intent's package references to participant directories,
/// validating along the way: unknown references and names matching several
/// projects are hard errors, and a release can only be demanded from a
/// participant — otherwise the intent could never be consumed and the file
/// would linger forever. A `none` decline is fine for any workspace package.
pub(super) fn resolve_intents(
    intents: &[ChangeIntent],
    refs: &ProjectRefIndex,
    participants: &BTreeMap<String, Participant<'_>>,
) -> Result<HashMap<String, BTreeMap<String, IntentBumpType>>, VersioningError> {
    let mut intent_bumps = HashMap::new();
    for intent in intents {
        let mut by_dir: BTreeMap<String, IntentBumpType> = BTreeMap::new();
        for (reference, bump_type) in &intent.releases {
            let dir = resolve_intent_ref(intent, refs, participants, reference, *bump_type)?;
            let entry = by_dir.entry(dir).or_insert(*bump_type);
            if bump_release_order(*bump_type) > bump_release_order(*entry) {
                *entry = *bump_type;
            }
        }
        intent_bumps.insert(intent.id.clone(), by_dir);
    }
    Ok(intent_bumps)
}

/// The participant directory one intent reference names.
fn resolve_intent_ref(
    intent: &ChangeIntent,
    refs: &ProjectRefIndex,
    participants: &BTreeMap<String, Participant<'_>>,
    reference: &str,
    bump_type: IntentBumpType,
) -> Result<String, VersioningError> {
    let dirs = refs.ref_to_dirs(reference);
    if dirs.is_empty() {
        return Err(VersioningError::UnknownPackage {
            file_path: intent.file_path.clone(),
            pkg_name: reference.to_string(),
        });
    }
    if dirs.len() > 1 {
        return Err(VersioningError::AmbiguousPackage {
            context: format!("Change intent file {}", intent.file_path.display()),
            reference: reference.to_string(),
            dirs,
        });
    }
    let dir = dirs.into_iter().next().expect("one element");
    if bump_type != IntentBumpType::None && !participants.contains_key(&dir) {
        return Err(VersioningError::UnreleasablePackage {
            file_path: intent.file_path.clone(),
            pkg_name: reference.to_string(),
            bump_type: bump_type.to_string(),
        });
    }
    Ok(dir)
}
