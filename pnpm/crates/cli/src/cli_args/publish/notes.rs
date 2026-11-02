//! The release notes `pnpm publish` sends to a registry declared
//! `serverType: pnpr`: a version note per package and, for a batch, the
//! publication's name and note. In a workspace that versions with pnpm in
//! `registry` changelog storage they come from the notes `pnpm version -r`
//! composed from the change intents, with any summary written by hand
//! already in them; elsewhere a package's version note is its
//! `CHANGELOG.md` section for the version, or `--release-notes`.

pub(super) use parked::check_packed_members;

use super::{PackedDirectory, PublishArgs, PublishScope};
use crate::cli_args::{
    changelog::published_name,
    release_notes::{notes_registries, notes_registry_of_manifest, read_release_notes_arg},
};
use derive_more::Display;
use miette::Context;
use parked::{parked_publication, publication_registry};
use pnpm_config::Config;
use pnpm_publish::{PublicationNotes, ReleaseNotesTarget};
use pnpm_release_notes::{
    find_changelog_section, is_publication_name, normalize_line_endings, validate_note,
};
use pnpm_reporter::{GlobalLog, LogEvent, LogLevel, Reporter};
use pnpm_versioning::{ChangelogStorage, changelog_storage};
use serde_json::Value;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

/// Where a note came from, as `pnpm publish` reports it.
#[derive(Debug, Display, Clone, Copy)]
enum NoteSource {
    #[display("--release-notes")]
    Option,
    #[display("change intents")]
    ChangeIntents,
    #[display("CHANGELOG.md")]
    Changelog,
}

/// A package's identity in its workspace: manifest name and version, the
/// key of the notes `pnpm version -r` parks.
pub(super) struct NotesPackage<'a> {
    pub dir: &'a Path,
    pub source_manifest: &'a Value,
    pub published_manifest: &'a Value,
}

impl PackedDirectory {
    pub(super) fn notes_package(&self) -> NotesPackage<'_> {
        NotesPackage {
            dir: &self.project_dir,
            source_manifest: &self.source_manifest,
            published_manifest: &self.published_manifest,
        }
    }
}

impl PublishArgs {
    /// A batch is one publication, whose name and note go to every registry
    /// that stores notes. Publishing one by one creates a publication per
    /// package, which has no note of its own.
    pub(super) fn attach_publication_notes<Reporter: self::Reporter>(
        &self,
        opts: &mut pnpm_publish::PublishPackedPkgOptions,
        graph: &pnpm_workspace_projects_filter::ProjectGraph<pnpm_workspace::GraphPkg<'_>>,
        to_publish: &HashSet<PathBuf>,
        config: &Config,
        dir: &Path,
    ) -> miette::Result<()> {
        if !self.flags.batch {
            if self.flags.manifest.notes.release_notes.is_some() {
                emit_warning::<Reporter>(
                    "--release-notes is not sent when packages are published one by one. Use --batch to publish them as one release with these notes.",
                );
            }
            return Ok(());
        }
        let members: Vec<&Value> = graph
            .iter()
            .filter(|(root, _)| to_publish.contains(*root))
            .map(|(_, node)| node.package.project.manifest.value())
            .collect();
        if !members
            .iter()
            .any(|manifest| notes_registry_of_manifest(config, manifest).is_some())
        {
            return Ok(());
        }
        opts.release_notes.publication = self.publication_notes::<Reporter>(config, dir, &members)?;
        Ok(())
    }

    /// Check the version note of every package in `to_publish` against the
    /// registry's content rules, so a note breaking them stops a
    /// recursive publish before its first upload rather than halfway
    /// through.
    pub(super) fn check_version_notes(
        &self,
        config: &Config,
        graph: &pnpm_workspace_projects_filter::ProjectGraph<pnpm_workspace::GraphPkg<'_>>,
        to_publish: &HashSet<PathBuf>,
    ) -> miette::Result<()> {
        if self.flags.manifest.notes.no_release_notes {
            return Ok(());
        }
        for (root, node) in graph
            .iter()
            .filter(|(root, _)| to_publish.contains(*root))
        {
            let manifest = node.package.project.manifest.value();
            if notes_registry_of_manifest(config, manifest).is_none() {
                continue;
            }
            let package =
                NotesPackage { dir: root, source_manifest: manifest, published_manifest: manifest };
            if let Some((text, source)) = version_note_source(config, &package, None)? {
                validate_note(&text)
                    .map_err(miette::Report::new)
                    .wrap_err_with(|| {
                        format!("Release notes of {} from {source}", package_id(manifest))
                    })?;
            }
        }
        Ok(())
    }

    /// Where the notes of this publish go.
    pub(super) fn release_notes_target(&self, config: &Config) -> ReleaseNotesTarget {
        if self.flags.manifest.notes.no_release_notes {
            return ReleaseNotesTarget::default();
        }
        ReleaseNotesTarget { registries: notes_registries(config), ..ReleaseNotesTarget::default() }
    }

    /// The text of `--release-notes`.
    pub(super) fn release_notes_text(&self, dir: &Path) -> miette::Result<Option<String>> {
        if self.flags.manifest.notes.no_release_notes {
            return Ok(None);
        }
        self.flags.manifest.notes.release_notes
            .as_deref()
            .map(|value| read_release_notes_arg(dir, value))
            .transpose()
    }

    /// The version note of a package bound for a registry that stores notes,
    /// checked against the registry's content rules. `None` when the
    /// package gets none.
    pub(super) fn version_notes<Reporter: self::Reporter>(
        &self,
        config: &Config,
        package: &NotesPackage<'_>,
        scope: PublishScope<'_>,
    ) -> miette::Result<Option<String>> {
        if self.flags.manifest.notes.no_release_notes
            || notes_registry_of_manifest(config, package.published_manifest).is_none()
        {
            return Ok(None);
        }
        let Some((text, source)) = version_note_source(config, package, scope.summary())? else {
            return Ok(None);
        };
        let id = package_id(package.published_manifest);
        checked_note::<Reporter>(&text, &id, source).map(Some)
    }

    /// The name and note of the publication a batch creates from the
    /// packages in `members` (manifests of the workspace).
    pub(super) fn publication_notes<Reporter: self::Reporter>(
        &self,
        config: &Config,
        dir: &Path,
        members: &[&Value],
    ) -> miette::Result<PublicationNotes> {
        if self.flags.manifest.notes.no_release_notes {
            return Ok(PublicationNotes::default());
        }
        let parked = parked_publication(config, members)?;
        let summary = self.release_notes_text(dir)?;
        if summary.is_some() && is_versioned(config, parked.as_ref(), members)? {
            return Err(summary_belongs_to_versioning());
        }
        let registry = parked
            .as_ref()
            .map(|publication| publication_registry(config, publication, members))
            .transpose()?
            .flatten();
        if parked.is_some() && registry.is_none() {
            // Its members no longer publish to a registry that stores notes,
            // so the composed publication goes nowhere rather than to every
            // such registry of the batch.
            emit_warning::<Reporter>(
                r#"The packages of the release that "pnpm version -r" composed release notes for no longer publish to a registry declared with serverType: pnpr. Its publication notes are not sent."#,
            );
            let name = self.publication_name(None)?;
            return Ok(PublicationNotes { name, ..PublicationNotes::default() });
        }
        let name = self.publication_name(parked.as_ref())?;
        let composed = parked
            .map(|publication| publication.notes)
            .filter(|notes| !notes.is_empty());
        let notes = summary
            .map(|summary| (summary, NoteSource::Option))
            .or_else(|| composed.map(|composed| (composed, NoteSource::ChangeIntents)));
        let notes = notes
            .map(|(text, source)| checked_note::<Reporter>(&text, "the publication", source))
            .transpose()?;
        Ok(PublicationNotes { name, notes, registry })
    }
}

impl PublishArgs {
    /// `--publication-name`, or else the name `pnpm version -r` gave
    /// `parked`, checked against the registry's naming rules.
    fn publication_name(
        &self,
        parked: Option<&pnpm_versioning::ParkedPublication>,
    ) -> miette::Result<Option<String>> {
        let name = self.flags.manifest.notes.publication_name
            .clone()
            .or_else(|| parked.and_then(|publication| publication.name.clone()));
        if let Some(name) = &name {
            validate_publication_name(name)?;
        }
        Ok(name)
    }

    /// The version note of a prebuilt tarball: `--release-notes`, or else
    /// the version's section of the `CHANGELOG.md` it packs.
    pub(super) fn tarball_notes<Reporter: self::Reporter>(
        &self,
        config: &Config,
        manifest: &Value,
        tarball: &[u8],
        summary: Option<&str>,
    ) -> miette::Result<Option<String>> {
        if self.flags.manifest.notes.no_release_notes
            || notes_registry_of_manifest(config, manifest).is_none()
        {
            return Ok(None);
        }
        let note = match summary {
            Some(summary) => Some((summary.to_owned(), NoteSource::Option)),
            None => {
                tarball_changelog_note(manifest, tarball).map(|text| (text, NoteSource::Changelog))
            }
        };
        note.map(|(text, source)| checked_note::<Reporter>(&text, &package_id(manifest), source))
            .transpose()
    }
}

fn tarball_changelog_note(manifest: &Value, tarball: &[u8]) -> Option<String> {
    let version = manifest.get("version")?.as_str()?;
    let changelog = crate::cli_args::changelog::extract_entry(tarball, "package/CHANGELOG.md")?;
    find_changelog_section(&normalize_line_endings(&changelog), version)
}

/// The version note of `package`: the one `pnpm version -r` composed, or
/// else `--release-notes` (`summary`), or else its `CHANGELOG.md` section.
/// A summary for a composed note belongs to `pnpm version -r`, which put it
/// in the note already.
fn version_note_source(
    config: &Config,
    package: &NotesPackage<'_>,
    summary: Option<&str>,
) -> miette::Result<Option<(String, NoteSource)>> {
    let parked = parked_version_note(config, package.source_manifest)?;
    Ok(match (summary, parked) {
        (Some(_), Some(_)) => return Err(summary_belongs_to_versioning()),
        (Some(summary), None) => Some((summary.to_owned(), NoteSource::Option)),
        (None, Some(parked)) => (!parked.is_empty()).then_some((parked, NoteSource::ChangeIntents)),
        (None, None) => changelog_note(package.dir, package.published_manifest)
            .map(|text| (text, NoteSource::Changelog)),
    })
}

/// Whether `pnpm version -r` composed notes for this batch: a publication,
/// or the version note of a package bound for a registry that stores
/// notes.
fn is_versioned(
    config: &Config,
    parked: Option<&pnpm_versioning::ParkedPublication>,
    members: &[&Value],
) -> miette::Result<bool> {
    if parked.is_some() {
        return Ok(true);
    }
    for manifest in members {
        if notes_registry_of_manifest(config, manifest).is_some()
            && parked_version_note(config, manifest)?.is_some()
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// `pnpm version -r` composes every note of a release it versioned, so the
/// digest it later checks covers the summary too.
fn summary_belongs_to_versioning() -> miette::Report {
    miette::miette!(
        code = "ERR_PNPM_RELEASE_NOTES_FROM_VERSIONING",
        help = r#"Give the summary to "pnpm version -r --release-notes", which composes it into the notes it parks for "pnpm publish"."#,
        r#"--release-notes cannot be given to "pnpm publish" for a release whose notes "pnpm version -r" composed"#,
    )
}

/// The version note `pnpm version -r` parked for the package, empty when it
/// composed none. `None` outside `registry` changelog storage. Failing to
/// read a parked file is an error, never an absent note, so no other source
/// takes its place.
fn parked_version_note(config: &Config, manifest: &Value) -> miette::Result<Option<String>> {
    let Some(workspace_dir) = config.workspace_dir.as_deref() else {
        return Ok(None);
    };
    if changelog_storage(Some(&config.versioning)) != ChangelogStorage::Registry {
        return Ok(None);
    }
    let name = manifest.get("name").and_then(Value::as_str);
    let version = manifest.get("version").and_then(Value::as_str);
    let (Some(name), Some(version)) = (name, version) else {
        return Ok(None);
    };
    let note = pnpm_versioning::read_pending_changelog(workspace_dir, name, version)?;
    Ok(note.map(|note| normalize_line_endings(&note)))
}

fn changelog_note(dir: &Path, manifest: &Value) -> Option<String> {
    let version = manifest.get("version")?.as_str()?;
    let changelog = std::fs::read_to_string(dir.join("CHANGELOG.md")).ok()?;
    find_changelog_section(&normalize_line_endings(&changelog), version)
}

/// `note` after the registry's content rules, reported with its source.
fn checked_note<Reporter: self::Reporter>(
    note: &str,
    subject: &str,
    source: NoteSource,
) -> miette::Result<String> {
    validate_note(note)
        .map_err(miette::Report::new)
        .wrap_err_with(|| format!("Release notes of {subject} from {source}"))?;
    Reporter::emit(&LogEvent::Global(GlobalLog {
        level: LogLevel::Info,
        message: format!("Release notes of {subject}: {source}"),
    }));
    Ok(note.to_owned())
}

fn package_id(manifest: &Value) -> String {
    let name = manifest
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let name = published_name(manifest).unwrap_or(name);
    let version = manifest
        .get("version")
        .and_then(Value::as_str)
        .unwrap_or_default();
    format!("{name}@{version}")
}

fn validate_publication_name(name: &str) -> miette::Result<()> {
    if is_publication_name(name) {
        return Ok(());
    }
    Err(miette::miette!(
        code = "ERR_PNPM_INVALID_PUBLICATION_NAME",
        help = r#"A publication name may contain letters, digits, ".", "_", "+", and "-", is at most 214 bytes long, and is not a number, which names a publication by its identifier."#,
        "Invalid publication name: {name:?}",
    ))
}

fn emit_warning<Reporter: self::Reporter>(message: &str) {
    Reporter::emit(&LogEvent::Global(GlobalLog {
        level: LogLevel::Warn,
        message: message.to_owned(),
    }));
}

mod parked;
