use super::{
    BTreeMap, BumpState, Identifier, IntentBumpType, Participant, PlannedRelease, Range,
    ReleaseBumpType, ResolvedEpic, Version, VersioningError, VersioningSettings,
    parse_workspace_spec_alias,
};

pub(super) fn bump_release_order(bump_type: IntentBumpType) -> u8 {
    match bump_type.release() {
        None => 0,
        Some(ReleaseBumpType::Patch) => 1,
        Some(ReleaseBumpType::Minor) => 2,
        Some(ReleaseBumpType::Major) => 3,
    }
}

pub(super) fn max_bump_type(
    types: impl Iterator<Item = IntentBumpType>,
) -> Option<ReleaseBumpType> {
    max_bump_type_of(types.filter_map(IntentBumpType::release))
}

pub(super) fn max_bump_type_of(
    types: impl Iterator<Item = ReleaseBumpType>,
) -> Option<ReleaseBumpType> {
    types.max()
}

pub(super) fn compute_new_version(
    current: &str,
    bump_type: ReleaseBumpType,
    lane_tag: Option<&str>,
    cumulative_bump: ReleaseBumpType,
    first_release: bool,
) -> String {
    let current_version = Version::parse(current).expect("participants have valid versions");
    let Some(lane_tag) = lane_tag else {
        return stable_release_version(
            current,
            &current_version,
            bump_type,
            cumulative_bump,
            first_release,
        );
    };
    if first_release {
        // A manifest prerelease already on this lane is published verbatim; a
        // stable (or off-lane) seed debuts at the lane's first prerelease.
        return if is_prerelease_on_lane(&current_version, lane_tag) {
            current.to_string()
        } else {
            format!("{}-{lane_tag}.0", stable_part(&current_version))
        };
    }
    let target = stable_target(&current_version, cumulative_bump);
    let next_n = next_prerelease_number(&current_version, &target, lane_tag);
    format!("{target}-{lane_tag}.{next_n}")
}

/// The version a package off every lane releases at.
fn stable_release_version(
    current: &str,
    current_version: &Version,
    bump_type: ReleaseBumpType,
    cumulative_bump: ReleaseBumpType,
    first_release: bool,
) -> String {
    if first_release {
        return current.to_string();
    }
    if current_version.pre_release.is_empty() {
        return inc_stable(current_version, bump_type);
    }
    // Graduation: the accumulated stable version the lane was building
    // toward.
    escalate_stable_target(&stable_part(current_version), cumulative_bump)
}

/// The stable version `bump_type` lands on, whether it bumps a stable
/// version or escalates the target a prerelease was building toward.
fn stable_target(current_version: &Version, bump_type: ReleaseBumpType) -> String {
    if current_version.pre_release.is_empty() {
        inc_stable(current_version, bump_type)
    } else {
        escalate_stable_target(&stable_part(current_version), bump_type)
    }
}

fn is_prerelease_on_lane(version: &Version, lane_tag: &str) -> bool {
    // semver parses an all-digit prerelease identifier as a number, so match the
    // tag comparison in `next_prerelease_number`.
    match version.pre_release.first() {
        Some(Identifier::AlphaNumeric(tag)) => tag == lane_tag,
        Some(Identifier::Numeric(tag)) => tag.to_string() == lane_tag,
        None => false,
    }
}

fn inc_stable(version: &Version, bump_type: ReleaseBumpType) -> String {
    match bump_type {
        ReleaseBumpType::Major => format!("{}.0.0", version.major + 1),
        ReleaseBumpType::Minor => format!("{}.{}.0", version.major, version.minor + 1),
        ReleaseBumpType::Patch => {
            format!("{}.{}.{}", version.major, version.minor, version.patch + 1)
        }
    }
}

/// Re-derives the stable version a lane is building toward when the
/// cumulative bump escalates. The invariant: the stable part of the current
/// prerelease already reflects the previous cumulative bump applied to the
/// version the lane started from, so only an escalation changes it.
fn escalate_stable_target(target: &str, cumulative_bump: ReleaseBumpType) -> String {
    let target_version = Version::parse(target).expect("stable parts are valid versions");
    match cumulative_bump {
        ReleaseBumpType::Major => {
            if target_version.minor == 0 && target_version.patch == 0 {
                target.to_string()
            } else {
                format!("{}.0.0", target_version.major + 1)
            }
        }
        ReleaseBumpType::Minor => {
            if target_version.patch == 0 {
                target.to_string()
            } else {
                format!("{}.{}.0", target_version.major, target_version.minor + 1)
            }
        }
        ReleaseBumpType::Patch => target.to_string(),
    }
}

fn stable_part(version: &Version) -> String {
    format!("{}.{}.{}", version.major, version.minor, version.patch)
}

fn next_prerelease_number(current: &Version, target: &str, lane_tag: &str) -> u64 {
    if current.pre_release.is_empty() || stable_part(current) != target {
        return 0;
    }
    // semver parses an all-digit prerelease identifier as a number, so the
    // tag comparison must not be strict about the identifier kind.
    let current_tag = match current.pre_release.first() {
        Some(Identifier::AlphaNumeric(tag)) => tag.clone(),
        Some(Identifier::Numeric(tag)) => tag.to_string(),
        None => return 0,
    };
    if current_tag != lane_tag {
        return 0;
    }
    match current.pre_release.get(1) {
        Some(Identifier::Numeric(current_n)) => current_n + 1,
        _ => 0,
    }
}

pub(super) fn apply_fixed_group_versions(
    participants: &BTreeMap<String, Participant<'_>>,
    state: &BTreeMap<String, BumpState>,
    new_versions: &mut BTreeMap<String, String>,
    cumulative_bump: &dyn Fn(&str, ReleaseBumpType) -> ReleaseBumpType,
    fixed_groups: &[Vec<String>],
    lanes_by_dir: &BTreeMap<String, String>,
) {
    for group in fixed_groups {
        let Some(group_bump) =
            max_bump_type_of(group.iter().filter_map(|dir| {
                state.get(dir).map(|entry| cumulative_bump(dir, entry.bump_type))
            }))
        else {
            continue;
        };
        let Some(shared_version) =
            shared_group_version(participants, group, group_bump, lanes_by_dir)
        else {
            continue;
        };
        for dir in group.iter().filter(|dir| state.contains_key(*dir)) {
            new_versions.insert(dir.clone(), shared_version.clone());
        }
    }
}

/// The one version every member of a fixed group releases at: the group's
/// highest current version, bumped, and on a lane numbered past every
/// member's own prerelease.
fn shared_group_version(
    participants: &BTreeMap<String, Participant<'_>>,
    group: &[String],
    group_bump: ReleaseBumpType,
    lanes_by_dir: &BTreeMap<String, String>,
) -> Option<String> {
    let current_of = |dir: &String| {
        Version::parse(participants[dir.as_str()].current_version)
            .expect("participants have valid versions")
    };
    let highest_current = group.iter().map(current_of).max()?;
    let target = stable_target(&highest_current, group_bump);

    let Some(lane_tag) = group.first().and_then(|dir| lanes_by_dir.get(dir)) else {
        return Some(target);
    };
    let next_n = group
        .iter()
        .map(|dir| next_prerelease_number(&current_of(dir), &target, lane_tag))
        .max()
        .unwrap_or(0);
    Some(format!("{target}-{lane_tag}.{next_n}"))
}

/// The band floor (`new_major × 100`) an epic re-bases its members to, or
/// `None` when no re-base is due. A re-base fires only when the lead releases
/// to a new, higher *stable* major in this plan; a prerelease lead version
/// (the lead on a lane) defers the re-base until its stable release.
pub(super) fn epic_rebase_floor(
    epic: &ResolvedEpic,
    participants: &BTreeMap<String, Participant<'_>>,
    new_versions: &BTreeMap<String, String>,
) -> Option<u64> {
    let lead = participants.get(epic.lead_dir.as_str())?;
    let new_lead = Version::parse(new_versions.get(&epic.lead_dir)?).ok()?;
    if !new_lead.pre_release.is_empty() {
        return None;
    }
    let current_major = epic_lead_band_major(&Version::parse(lead.current_version).ok()?);
    (new_lead.major > current_major).then_some(new_lead.major * 100)
}

fn epic_lead_band_major(version: &Version) -> u64 {
    if !version.pre_release.is_empty() && version.minor == 0 && version.patch == 0 {
        version.major.saturating_sub(1)
    } else {
        version.major
    }
}

/// Overrides the computed version of every bumped epic member with the band
/// floor when its lead crosses to a new stable major. A member on a lane
/// re-bases to a prerelease of the floor; every other member to `floor.0.0`.
pub(super) fn apply_epic_band_versions(
    participants: &BTreeMap<String, Participant<'_>>,
    state: &BTreeMap<String, BumpState>,
    new_versions: &mut BTreeMap<String, String>,
    epics: &[ResolvedEpic],
    lanes_by_dir: &BTreeMap<String, String>,
) {
    for epic in epics {
        let Some(floor) = epic_rebase_floor(epic, participants, new_versions) else {
            continue;
        };
        let target = format!("{floor}.0.0");
        for member_dir in &epic.member_dirs {
            if !state.contains_key(member_dir) {
                continue;
            }
            let version = match lanes_by_dir.get(member_dir) {
                None => target.clone(),
                Some(lane_tag) => {
                    let current = Version::parse(participants[member_dir.as_str()].current_version)
                        .expect("participants have valid versions");
                    let next_n = next_prerelease_number(&current, &target, lane_tag);
                    format!("{target}-{lane_tag}.{next_n}")
                }
            };
            new_versions.insert(member_dir.clone(), version);
        }
    }
}

/// The band of member majors an epic permits, `[lead_major*100,
/// lead_major*100+99]`, where `lead_major` is the major the plan establishes
/// for the lead — its re-based major when the lead crosses to a new stable
/// major, otherwise the lead's current major (a prerelease lead does not open
/// the next band).
pub(super) struct EpicBand {
    pub(super) major: u64,
    pub(super) low: u64,
    pub(super) high: u64,
}

impl EpicBand {
    pub(super) fn contains(&self, member_major: u64) -> bool {
        (self.low..=self.high).contains(&member_major)
    }
}

pub(super) fn epic_band(
    epic: &ResolvedEpic,
    participants: &BTreeMap<String, Participant<'_>>,
    new_versions: &BTreeMap<String, String>,
) -> EpicBand {
    let major = match epic_rebase_floor(epic, participants, new_versions) {
        Some(floor) => floor / 100,
        None => epic_lead_band_major(
            &Version::parse(participants[epic.lead_dir.as_str()].current_version)
                .expect("participants have valid versions"),
        ),
    };
    let low = major * 100;
    EpicBand { major, low, high: low + 99 }
}

/// Enforces that every released member's new major stays inside its epic's
/// band. The re-base already keeps members in band when the lead moves; this
/// guards the other direction — an ordinary `major` intent that would carry a
/// member over the band ceiling (`1199.x` -> `1200.0.0` while the lead is
/// still on 11) is rejected rather than silently landing in the next band.
pub(super) fn enforce_epic_bands(
    epics: &[ResolvedEpic],
    participants: &BTreeMap<String, Participant<'_>>,
    new_versions: &BTreeMap<String, String>,
) -> Result<(), VersioningError> {
    for epic in epics {
        let band = epic_band(epic, participants, new_versions);
        for member_dir in &epic.member_dirs {
            let Some(member_version) = new_versions.get(member_dir) else {
                continue;
            };
            let member_major =
                Version::parse(member_version).expect("participants have valid versions").major;
            if !band.contains(member_major) {
                return Err(VersioningError::EpicOutOfBand {
                    pkg_name: participants[member_dir.as_str()].name.to_string(),
                    new_version: member_version.clone(),
                    member_major,
                    lead: epic.lead_ref.clone(),
                    band_major: band.major,
                });
            }
        }
    }
    Ok(())
}

/// The range that pnpm materializes for a workspace: spec at pack time,
/// given the dependency's version at the dependent's previous release.
/// Dependent propagation republishes the dependent whenever the dependency's
/// new version falls outside this range.
#[must_use]
pub fn materialize_workspace_range(spec: &str, dep_current_version: &str) -> Option<String> {
    let rest = spec.strip_prefix("workspace:")?;
    let range = match parse_workspace_spec_alias(rest) {
        Some(alias) => &rest[alias.len() + 1..],
        None => rest,
    };
    Some(match range {
        "^" => format!("^{dep_current_version}"),
        "~" => format!("~{dep_current_version}"),
        "*" | "" => dep_current_version.to_string(),
        explicit => explicit.to_string(),
    })
}

pub(super) fn range_accepts(range: &str, version: &str) -> bool {
    let (Ok(range), Ok(version)) = (Range::parse(range), Version::parse(version)) else {
        return false;
    };
    version.satisfies(&range)
}

pub(super) fn enforce_max_bump(
    releases: &[PlannedRelease],
    versioning: Option<&VersioningSettings>,
) -> Result<(), VersioningError> {
    let Some(max_bump) = versioning.and_then(|settings| settings.max_bump) else {
        return Ok(());
    };
    for release in releases {
        let effective_bump = effective_bump_class(release);
        if effective_bump <= max_bump {
            continue;
        }
        let intent_files: Vec<String> = release
            .intents
            .iter()
            .filter(|intent| {
                intent.releases.values().any(|bump| bump.release() == Some(effective_bump))
            })
            .map(|intent| intent.file_path.display().to_string())
            .collect();
        let raised_by = if intent_files.is_empty() {
            format!(
                "constraint chain: {}",
                release.causes.iter().map(ToString::to_string).collect::<Vec<String>>().join(", "),
            )
        } else {
            format!("intent file(s) {}", intent_files.join(", "))
        };
        return Err(VersioningError::MaxBumpExceeded {
            pkg_name: release.name.clone(),
            bump_type: effective_bump.to_string(),
            max_bump: max_bump.to_string(),
            raised_by,
        });
    }
    Ok(())
}

/// The bump class a release actually applies. Fixed-group version sharing
/// and lane escalation can move a version further than the package's own
/// declared or propagated bump, so the cap compares against the real
/// distance between the current and the new version as well.
fn effective_bump_class(release: &PlannedRelease) -> ReleaseBumpType {
    let (Ok(current), Ok(new_version)) =
        (Version::parse(&release.current_version), Version::parse(&release.new_version))
    else {
        return release.bump_type;
    };
    let diff_class = if new_version.major != current.major {
        Some(ReleaseBumpType::Major)
    } else if new_version.minor != current.minor {
        Some(ReleaseBumpType::Minor)
    } else if new_version.patch != current.patch {
        Some(ReleaseBumpType::Patch)
    } else {
        None
    };
    diff_class.into_iter().chain([release.bump_type]).max().unwrap_or(release.bump_type)
}
