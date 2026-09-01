use super::{
    InteractiveUpdateProject, PromptRow, UpdatePrompt, collect_choices, dependencies_prompt_message,
};
use crate::cli_args::update::UpdateArgs;
use clap::Parser;
use pnpm_config::Config;
use pnpm_lockfile::Lockfile;
use pnpm_network::ThrottledClient;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_reporter::{GlobalLog, LogEvent, LogLevel, Reporter, SilentReporter};
use pnpm_testing_utils::registry::TestRegistry;
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tempfile::TempDir;

const TEST_INTEGRITY: &str = "sha512-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa==";
/// The default release-age policy fetches abbreviated metadata, then upgrades
/// it to full metadata when this test fixture omits publish times.
const RELEASE_AGE_METADATA_REQUESTS: usize = 2;

#[tokio::test]
async fn interactive_choices_respect_minimum_release_age() {
    let temp = tempfile::tempdir().expect("create temporary workspace");
    let manifest = manifest_with_dependency(temp.path(), "packages/a", "foo");
    let lockfile: Lockfile = serde_saphyr::from_str(
        r"
lockfileVersion: '9.0'
importers:
  packages/a:
    dependencies:
      foo:
        specifier: ^1.0.0
        version: 1.0.0
",
    )
    .expect("parse workspace lockfile");
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let foo_mock = server
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(package_body_with_publish_times("foo", &registry))
        .create_async()
        .await;
    let mut config = Config::new();
    config.registry = registry;
    config.cache_dir = temp.path().join("cache");
    config.minimum_release_age = Some(60);
    let projects =
        [InteractiveUpdateProject { manifest: &manifest, importer_id: "packages/a".to_string() }];

    let choices = collect_choices(
        &projects,
        Some(&lockfile),
        &config,
        &Arc::new(ThrottledClient::default()),
        true,
        &[DependencyGroup::Prod],
    )
    .await
    .expect("collect interactive choices");

    assert_eq!(choices.len(), 1);
    assert_eq!(choices[0].target.to_string(), "1.0.1");
    foo_mock.assert_async().await;
}

#[tokio::test]
async fn collects_choices_from_each_selected_workspace_importer() {
    let temp = tempfile::tempdir().expect("create temporary workspace");
    let foo = manifest_with_dependency(temp.path(), "packages/a", "foo");
    let bar = manifest_with_dependency(temp.path(), "packages/b", "bar");
    let lockfile: Lockfile = serde_saphyr::from_str(
        r"
lockfileVersion: '9.0'
importers:
  packages/a:
    dependencies:
      foo:
        specifier: ^1.0.0
        version: 1.0.0
  packages/b:
    dependencies:
      bar:
        specifier: ^1.0.0
        version: 1.0.0
",
    )
    .expect("parse workspace lockfile");
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let foo_mock = server
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(package_body("foo", &registry))
        .expect(RELEASE_AGE_METADATA_REQUESTS)
        .create_async()
        .await;
    let bar_mock = server
        .mock("GET", "/bar")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(package_body("bar", &registry))
        .expect(RELEASE_AGE_METADATA_REQUESTS)
        .create_async()
        .await;
    let mut config = Config::new();
    config.registry = registry;
    config.cache_dir = temp.path().join("cache");
    let projects = [
        InteractiveUpdateProject { manifest: &foo, importer_id: "packages/a".to_string() },
        InteractiveUpdateProject { manifest: &bar, importer_id: "packages/b".to_string() },
    ];

    let choices = collect_choices(
        &projects,
        Some(&lockfile),
        &config,
        &Arc::new(ThrottledClient::default()),
        false,
        &[DependencyGroup::Prod],
    )
    .await
    .expect("collect interactive choices");

    assert_eq!(
        choices.iter().map(|choice| choice.alias.as_str()).collect::<Vec<_>>(),
        vec!["foo", "bar"],
    );
    // Each entry remembers the project it came from, which is what the
    // interactive list's `Workspace` column shows.
    assert_eq!(
        choices.iter().map(|choice| choice.workspace.as_deref()).collect::<Vec<_>>(),
        vec![Some("packages-a"), Some("packages-b")],
    );
    foo_mock.assert_async().await;
    bar_mock.assert_async().await;
}

#[tokio::test]
async fn keeps_distinct_aliases_for_the_same_package() {
    let temp = tempfile::tempdir().expect("create temporary workspace");
    let direct = manifest_with_dependency_spec(temp.path(), "packages/a", ("foo", "^1.0.0"));
    let alias =
        manifest_with_dependency_spec(temp.path(), "packages/b", ("fooAlias", "npm:foo@^1.0.0"));
    let lockfile: Lockfile = serde_saphyr::from_str(
        r"
lockfileVersion: '9.0'
importers:
  packages/a:
    dependencies:
      foo:
        specifier: ^1.0.0
        version: 1.0.0
  packages/b:
    dependencies:
      fooAlias:
        specifier: npm:foo@^1.0.0
        version: foo@1.0.0
",
    )
    .expect("parse workspace lockfile");
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let foo_mock = server
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(package_body("foo", &registry))
        .expect(RELEASE_AGE_METADATA_REQUESTS)
        .create_async()
        .await;
    let mut config = Config::new();
    config.registry = registry;
    config.cache_dir = temp.path().join("cache");
    let projects = [
        InteractiveUpdateProject { manifest: &direct, importer_id: "packages/a".to_string() },
        InteractiveUpdateProject { manifest: &alias, importer_id: "packages/b".to_string() },
    ];

    let choices = collect_choices(
        &projects,
        Some(&lockfile),
        &config,
        &Arc::new(ThrottledClient::default()),
        false,
        &[DependencyGroup::Prod],
    )
    .await
    .expect("collect interactive choices");

    assert_eq!(
        choices.iter().map(|choice| choice.alias.as_str()).collect::<Vec<_>>(),
        vec!["foo", "fooAlias"],
    );
    foo_mock.assert_async().await;
}

/// The same dependency at the same version in two projects has to reach
/// [`super::choices::update_choices`] as two entries, or the collapsed
/// row it renders can only name one of the projects.
#[tokio::test]
async fn one_dependency_in_two_projects_keeps_both_workspaces() {
    let temp = tempfile::tempdir().expect("create temporary workspace");
    let first = manifest_with_dependency(temp.path(), "packages/a", "foo");
    let second = manifest_with_dependency(temp.path(), "packages/b", "foo");
    let lockfile: Lockfile = serde_saphyr::from_str(
        r"
lockfileVersion: '9.0'
importers:
  packages/a:
    dependencies:
      foo:
        specifier: ^1.0.0
        version: 1.0.0
  packages/b:
    dependencies:
      foo:
        specifier: ^1.0.0
        version: 1.0.0
",
    )
    .expect("parse workspace lockfile");
    let mut server = mockito::Server::new_async().await;
    let registry = format!("{}/", server.url());
    let foo_mock = server
        .mock("GET", "/foo")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(package_body("foo", &registry))
        .expect(RELEASE_AGE_METADATA_REQUESTS)
        .create_async()
        .await;
    let mut config = Config::new();
    config.registry = registry;
    config.cache_dir = temp.path().join("cache");
    let projects = [
        InteractiveUpdateProject { manifest: &first, importer_id: "packages/a".to_string() },
        InteractiveUpdateProject { manifest: &second, importer_id: "packages/b".to_string() },
    ];

    let choices = collect_choices(
        &projects,
        Some(&lockfile),
        &config,
        &Arc::new(ThrottledClient::default()),
        false,
        &[DependencyGroup::Prod],
    )
    .await
    .expect("collect interactive choices");

    assert_eq!(
        choices.iter().map(|choice| choice.workspace.as_deref()).collect::<Vec<_>>(),
        vec![Some("packages-a"), Some("packages-b")],
    );
    // And they render as one row naming both.
    let groups = super::choices::update_choices(&choices.iter().collect::<Vec<_>>(), true);
    assert!(
        groups[0].rows[1].label.contains("packages-a, packages-b"),
        "{}",
        groups[0].rows[1].label,
    );
    foo_mock.assert_async().await;
}

/// A project may omit its `name` or declare it empty; either way the
/// label would be blank, leaving several such projects indistinguishable
/// in the interactive list, so the entry falls back to the path that
/// identifies the project in the lockfile.
#[tokio::test]
async fn a_project_without_a_usable_name_is_labelled_with_its_importer_path() {
    for name in [None, Some(""), Some("   ")] {
        let temp = tempfile::tempdir().expect("create temporary workspace");
        let manifest = manifest_without_usable_name(temp.path(), "packages/a", name);
        let lockfile: Lockfile = serde_saphyr::from_str(
            r"
lockfileVersion: '9.0'
importers:
  packages/a:
    dependencies:
      foo:
        specifier: ^1.0.0
        version: 1.0.0
",
        )
        .expect("parse workspace lockfile");
        let mut server = mockito::Server::new_async().await;
        let registry = format!("{}/", server.url());
        let foo_mock = server
            .mock("GET", "/foo")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(package_body("foo", &registry))
            .expect(RELEASE_AGE_METADATA_REQUESTS)
            .create_async()
            .await;
        let mut config = Config::new();
        config.registry = registry;
        config.cache_dir = temp.path().join("cache");
        let projects = [InteractiveUpdateProject {
            manifest: &manifest,
            importer_id: "packages/a".to_string(),
        }];

        let choices = collect_choices(
            &projects,
            Some(&lockfile),
            &config,
            &Arc::new(ThrottledClient::default()),
            false,
            &[DependencyGroup::Prod],
        )
        .await
        .expect("collect interactive choices");

        assert_eq!(
            choices.iter().map(|choice| choice.workspace.as_deref()).collect::<Vec<_>>(),
            vec![Some("packages/a")],
            "a {name:?} name should fall back to the importer path",
        );
        foo_mock.assert_async().await;
    }
}

fn manifest_with_dependency(
    root: &std::path::Path,
    relative: &str,
    dependency: &str,
) -> PackageManifest {
    manifest_with_dependency_spec(root, relative, (dependency, "^1.0.0"))
}

fn manifest_with_dependency_spec(
    root: &std::path::Path,
    relative: &str,
    dependency: (&str, &str),
) -> PackageManifest {
    let (dependency, specifier) = dependency;
    let project_dir = root.join(relative);
    std::fs::create_dir_all(&project_dir).expect("create project directory");
    let manifest_path = project_dir.join("package.json");
    std::fs::write(
        &manifest_path,
        json!({
            "name": relative.replace('/', "-"),
            "dependencies": { dependency: specifier },
        })
        .to_string(),
    )
    .expect("write project manifest");
    PackageManifest::from_path(manifest_path).expect("read project manifest")
}

/// A project manifest whose `name` is absent, or present but empty —
/// both shapes a workspace project's `package.json` can carry.
fn manifest_without_usable_name(
    root: &std::path::Path,
    relative: &str,
    name: Option<&str>,
) -> PackageManifest {
    let project_dir = root.join(relative);
    std::fs::create_dir_all(&project_dir).expect("create project directory");
    let manifest_path = project_dir.join("package.json");
    let mut manifest = json!({ "dependencies": { "foo": "^1.0.0" } });
    if let Some(name) = name {
        manifest["name"] = json!(name);
    }
    std::fs::write(&manifest_path, manifest.to_string()).expect("write project manifest");
    PackageManifest::from_path(manifest_path).expect("read project manifest")
}

fn package_body(name: &str, registry: &str) -> String {
    let version = |version: &str| {
        json!({
            "name": name,
            "version": version,
            "dist": {
                "integrity": TEST_INTEGRITY,
                "tarball": format!("{registry}{name}/-/{name}-{version}.tgz"),
            },
        })
    };
    json!({
        "name": name,
        "dist-tags": { "latest": "1.1.0" },
        "versions": {
            "1.0.0": version("1.0.0"),
            "1.1.0": version("1.1.0"),
        },
    })
    .to_string()
}

fn package_body_with_publish_times(name: &str, registry: &str) -> String {
    let mut body: serde_json::Value =
        serde_json::from_str(&package_body(name, registry)).expect("parse package body");
    body["versions"]["1.0.1"] = json!({
        "name": name,
        "version": "1.0.1",
        "dist": {
            "integrity": TEST_INTEGRITY,
            "tarball": format!("{registry}{name}/-/{name}-1.0.1.tgz"),
        },
    });
    body["time"] = json!({
        "1.0.0": "2020-01-01T00:00:00.000Z",
        "1.0.1": "2020-02-01T00:00:00.000Z",
        "1.1.0": "2999-01-01T00:00:00.000Z",
    });
    body.to_string()
}

mod selection {
    use super::super::{
        choices::{ChoiceGroup, ChoiceRow},
        flatten_groups, selected_packages,
    };

    fn group(message: &str, rows: &[(&str, Option<&str>)]) -> ChoiceGroup {
        ChoiceGroup {
            message: message.to_string(),
            rows: rows
                .iter()
                .map(|(label, value)| ChoiceRow {
                    label: (*label).to_string(),
                    value: value.map(str::to_string),
                })
                .collect(),
        }
    }

    /// Checking a group heading or a column header updates nothing —
    /// `dialoguer` lets either be checked, where pnpm's prompt disables
    /// them outright.
    #[test]
    fn headings_and_headers_select_nothing() {
        let groups =
            [group("dependencies", &[("Package Current", None), ("foo 1 ❯ 2", Some("foo"))])];

        let rows = flatten_groups(&groups);

        assert_eq!(rows.len(), 3, "heading, header row, and one package");
        // 0 is the heading, 1 the column header, 2 the package.
        assert_eq!(selected_packages(&rows, &[0, 1]), Vec::<String>::new());
        assert_eq!(selected_packages(&rows, &[2]), vec!["foo".to_string()]);
    }

    /// The same package offered by two importers is returned once, in the
    /// order it was first checked.
    #[test]
    fn a_package_checked_twice_is_returned_once() {
        let groups = [
            group("dependencies", &[("hdr", None), ("foo", Some("foo"))]),
            group("devDependencies", &[("hdr", None), ("bar", Some("bar")), ("foo", Some("foo"))]),
        ];

        let rows = flatten_groups(&groups);

        // 2 = foo (prod), 5 = bar, 6 = foo (dev).
        assert_eq!(
            selected_packages(&rows, &[2, 5, 6]),
            vec!["foo".to_string(), "bar".to_string()],
        );
    }

    /// An out-of-range index cannot panic the selection.
    #[test]
    fn an_unknown_index_is_ignored() {
        let groups = [group("dependencies", &[("hdr", None), ("foo", Some("foo"))])];

        let rows = flatten_groups(&groups);

        assert_eq!(selected_packages(&rows, &[99]), Vec::<String>::new());
    }
}

// ---------------------------------------------------------------------
// The scripted prompt behind [`UpdatePrompt::Scripted`], and the ports of
// `pnpm11/installing/commands/test/update/interactive.ts` it carries.
// ---------------------------------------------------------------------

/// What one prompt put in front of the user.
struct SeenPrompt {
    message: String,
    /// The label of each row, paired with the package checking it
    /// updates — [`None`] for a group heading or a header row.
    rows: Vec<(String, Option<String>)>,
}

/// How a test answers one prompt: the packages to check, or Ctrl-C.
enum ScriptedAnswer {
    Check(Vec<String>),
    Cancel,
}

struct PromptScript {
    /// One entry per prompt, in call order.
    answers: VecDeque<ScriptedAnswer>,
    seen: Vec<SeenPrompt>,
}

static SCRIPT: Mutex<PromptScript> =
    Mutex::new(PromptScript { answers: VecDeque::new(), seen: Vec::new() });

fn script() -> std::sync::MutexGuard<'static, PromptScript> {
    SCRIPT.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

static SCRIPT_LOCK: Mutex<()> = Mutex::new(());

/// One test's claim on the scripted prompt.
///
/// [`SCRIPT`] is process-global, which `cargo nextest run` — a process
/// per test — makes invisible, but `cargo test` does not. Holding this
/// for the test's lifetime keeps two tests from consuming each other's
/// answers.
struct ScriptedPrompts {
    script: &'static Mutex<PromptScript>,
    _claim: std::sync::MutexGuard<'static, ()>,
}

/// Claim the scripted prompt, with nothing answered and nothing seen.
fn scripted_prompts() -> ScriptedPrompts {
    let claim = SCRIPT_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut script = script();
    script.answers.clear();
    script.seen.clear();
    drop(script);
    ScriptedPrompts { script: &SCRIPT, _claim: claim }
}

impl ScriptedPrompts {
    /// Answer the next prompt by checking the rows for these packages,
    /// the way the upstream suite resolves its `@inquirer/prompts` mock.
    fn answer_next(&self, packages: &[&str]) {
        let answer = packages.iter().map(|package| (*package).to_string()).collect();
        self.claimed().answers.push_back(ScriptedAnswer::Check(answer));
    }

    /// Leave the next prompt with Ctrl-C.
    fn cancel_next(&self) {
        self.claimed().answers.push_back(ScriptedAnswer::Cancel);
    }

    /// Take the prompts shown since the last call.
    fn seen(&self) -> Vec<SeenPrompt> {
        std::mem::take(&mut self.claimed().seen)
    }

    fn claimed(&self) -> std::sync::MutexGuard<'static, PromptScript> {
        self.script.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

pub(super) fn answer_prompt(message: &str, rows: &[PromptRow]) -> Option<Vec<usize>> {
    let mut script = script();
    let answer = script
        .answers
        .pop_front()
        .unwrap_or_else(|| panic!("the test scripted no answer for the prompt {message:?}"));
    script.seen.push(SeenPrompt {
        message: message.to_string(),
        rows: rows
            .iter()
            .map(|row| match row {
                PromptRow::Separator(text) => (text.clone(), None),
                PromptRow::Choice { label, value, .. } => (label.clone(), Some(value.clone())),
            })
            .collect(),
    });
    let ScriptedAnswer::Check(answer) = answer else { return None };
    Some(
        rows.iter()
            .enumerate()
            .filter(|(_, row)| {
                matches!(row, PromptRow::Choice { value, .. } if answer.iter().any(|name| name == value))
            })
            .map(|(index, _)| index)
            .collect(),
    )
}

/// `(package, current, target)` for every row the user could check. The
/// versions are read back out of the padded label; how that table is laid
/// out is pinned by the `choices` ports.
fn offered(prompt: &SeenPrompt) -> Vec<(String, String, String)> {
    prompt
        .rows
        .iter()
        .filter_map(|(label, value)| {
            let package = value.as_ref()?;
            let columns = label.split_whitespace().collect::<Vec<_>>();
            let arrow = columns.iter().position(|column| *column == "❯")?;
            Some((package.clone(), columns[arrow - 1].to_string(), columns[arrow + 1].to_string()))
        })
        .collect()
}

/// The group headings a prompt showed, in order: the separators drawn
/// as `── heading ──`.
fn headings(prompt: &SeenPrompt) -> Vec<String> {
    prompt
        .rows
        .iter()
        .filter(|(_, value)| value.is_none())
        .filter_map(|(label, _)| {
            console::strip_ansi_codes(label)
                .strip_prefix("── ")?
                .strip_suffix(" ──")
                .map(str::to_string)
        })
        .collect()
}

/// Published at 1.0.0, 1.0.1, 2.0.0, and 2.1.0.
const MULTI_A: &str = "@pnpm.e2e/multi-version-a";
/// Published at 1.0.0, 2.0.0, 3.0.0, and 3.1.0.
const MULTI_B: &str = "@pnpm.e2e/multi-version-b";
/// Published at 3.0.0, 3.1.10, and 4.0.0.
const MULTI_C: &str = "@pnpm.e2e/multi-version-c";

/// A single-project workspace against a mocked registry of its own,
/// driven through the same `UpdateArgs` the CLI dispatches.
struct UpdateFixture {
    _dir: TempDir,
    project: PathBuf,
    cache_dir: PathBuf,
    config: &'static Config,
    registry: TestRegistry,
}

impl UpdateFixture {
    fn new() -> Self {
        Self::with_config(|_| {})
    }

    fn with_config(customize: impl FnOnce(&mut Config)) -> Self {
        let dir = tempfile::tempdir().expect("create temporary workspace");
        let registry = TestRegistry::start_with_own_storage(dir.path());
        let project = dir.path().join("project");
        fs::create_dir_all(&project).expect("create the project dir");
        let cache_dir = dir.path().join("cache");
        let mut config = Config::new();
        config.registry = registry.url();
        config.store_dir = dir.path().join("store").into();
        config.cache_dir = cache_dir.clone();
        config.modules_dir = project.join("node_modules");
        config.virtual_store_dir = project.join("node_modules/.pnpm");
        config.enable_global_virtual_store = false;
        customize(&mut config);
        let config = Config::leak(config);
        Self { _dir: dir, project, cache_dir, config, registry }
    }

    fn write_manifest(&self, dependencies: &Value) {
        let manifest =
            json!({ "name": "project", "version": "1.0.0", "dependencies": dependencies });
        fs::write(self.project.join("package.json"), manifest.to_string())
            .expect("write package.json");
    }

    /// Move a dist tag, and drop the packument the last run cached so the
    /// next one resolves against the move — the same pairing as
    /// `AddMockedRegistry::set_dist_tag`.
    fn set_dist_tag(&self, package: &str, version: &str, tag: &str) {
        self.registry.set_dist_tag(package, version, tag);
        if self.cache_dir.exists() {
            fs::remove_dir_all(&self.cache_dir).expect("drop the cached registry metadata");
        }
    }

    async fn update(&self, args: &[&str]) {
        self.update_reporting::<SilentReporter>(args).await;
    }

    async fn update_reporting<Reporter: self::Reporter>(&self, args: &[&str]) {
        #[derive(Parser)]
        struct Harness {
            #[clap(flatten)]
            args: UpdateArgs,
        }
        let mut parsed =
            Harness::try_parse_from(std::iter::once("pacquet-test").chain(args.iter().copied()))
                .expect("parse update arguments")
                .args;
        parsed.prompt = UpdatePrompt::Scripted;
        let state = crate::State::init(self.project.join("package.json"), self.config, false)
            .expect("initialize the state");
        parsed.run::<Reporter>(state).await.expect("run pacquet update");
    }

    /// The `packages:` keys of the lockfile the last run wrote.
    fn lockfile_packages(&self) -> Vec<String> {
        let text = fs::read_to_string(self.project.join("pnpm-lock.yaml"))
            .expect("read the wanted lockfile");
        let lockfile: Lockfile = serde_saphyr::from_str(&text).expect("parse the wanted lockfile");
        let mut keys = lockfile
            .packages
            .into_iter()
            .flatten()
            .map(|(key, _)| key.to_string())
            .collect::<Vec<_>>();
        keys.sort();
        keys
    }
}

/// Ports `interactively update`.
#[tokio::test]
async fn interactively_update() {
    let fixture = UpdateFixture::new();
    fixture.set_dist_tag(MULTI_A, "2.1.0", "latest");
    fixture.set_dist_tag(MULTI_C, "4.0.0", "latest");

    fixture.write_manifest(&json!({ MULTI_A: "1.0.0", MULTI_B: "2.0.0", MULTI_C: "3.0.0" }));
    fixture.update(&["update"]).await;
    fixture.write_manifest(&json!({ MULTI_A: "^1.0.0", MULTI_B: "^2.0.0", MULTI_C: "^3.0.0" }));

    let scripted = scripted_prompts();
    scripted.answer_next(&[MULTI_A]);
    fixture.update(&["update", "--interactive"]).await;

    let prompts = scripted.seen();
    assert_eq!(prompts.len(), 1);
    assert_eq!(prompts[0].message, dependencies_prompt_message());
    assert_eq!(headings(&prompts[0]), ["dependencies"]);
    assert_eq!(
        offered(&prompts[0]),
        [
            (MULTI_A.to_string(), "1.0.0".to_string(), "1.0.1".to_string()),
            (MULTI_C.to_string(), "3.0.0".to_string(), "3.1.10".to_string()),
        ],
    );
    assert_eq!(
        fixture.lockfile_packages(),
        [format!("{MULTI_A}@1.0.1"), format!("{MULTI_B}@2.0.0"), format!("{MULTI_C}@3.0.0")],
    );

    scripted.answer_next(&[MULTI_A]);
    fixture.update(&["update", "--interactive", "--latest"]).await;

    let prompts = scripted.seen();
    assert_eq!(prompts.len(), 1);
    assert_eq!(
        offered(&prompts[0]),
        [
            (MULTI_A.to_string(), "1.0.1".to_string(), "2.1.0".to_string()),
            (MULTI_B.to_string(), "2.0.0".to_string(), "3.1.0".to_string()),
            (MULTI_C.to_string(), "3.0.0".to_string(), "4.0.0".to_string()),
        ],
    );
    assert_eq!(
        fixture.lockfile_packages(),
        [format!("{MULTI_A}@2.1.0"), format!("{MULTI_B}@2.0.0"), format!("{MULTI_C}@3.0.0")],
    );
}

/// Ports `interactively update should ignore dependencies from the
/// ignoreDependencies field`.
#[tokio::test]
async fn interactively_update_skips_ignored_dependencies() {
    let fixture = UpdateFixture::with_config(|config| {
        config.update_config.ignore_dependencies = Some(vec![MULTI_A.to_string()]);
    });

    fixture.write_manifest(&json!({ MULTI_A: "1.0.0", MULTI_B: "2.0.0", MULTI_C: "3.0.0" }));
    fixture.update(&["update"]).await;
    fixture.write_manifest(&json!({ MULTI_A: "^1.0.0", MULTI_B: "^2.0.0", MULTI_C: "^3.0.0" }));

    let scripted = scripted_prompts();
    scripted.answer_next(&[MULTI_C]);
    fixture.update(&["update", "--interactive"]).await;

    let prompts = scripted.seen();
    assert_eq!(prompts.len(), 1);
    assert_eq!(
        offered(&prompts[0]),
        [(MULTI_C.to_string(), "3.0.0".to_string(), "3.1.10".to_string())],
    );
    assert_eq!(
        fixture.lockfile_packages(),
        [format!("{MULTI_A}@1.0.0"), format!("{MULTI_B}@2.0.0"), format!("{MULTI_C}@3.1.10")],
    );
}

/// Ports `global interactive update leaves without an error when the
/// prompt is canceled`, for the dependency prompt: Ctrl-C is how the
/// user declines, so the command reports it and leaves with nothing
/// updated and no error.
#[tokio::test]
async fn interactive_update_leaves_without_an_error_when_the_prompt_is_canceled() {
    static EVENTS: Mutex<Vec<LogEvent>> = Mutex::new(Vec::new());
    struct RecordingReporter;
    impl Reporter for RecordingReporter {
        fn emit(event: &LogEvent) {
            EVENTS.lock().unwrap().push(event.clone());
        }
    }

    let fixture = UpdateFixture::new();
    fixture.write_manifest(&json!({ MULTI_A: "1.0.0" }));
    fixture.update(&["update"]).await;
    fixture.write_manifest(&json!({ MULTI_A: "^1.0.0" }));

    let scripted = scripted_prompts();
    scripted.cancel_next();
    fixture.update_reporting::<RecordingReporter>(&["update", "--interactive"]).await;

    assert_eq!(scripted.seen().len(), 1);
    assert_eq!(fixture.lockfile_packages(), [format!("{MULTI_A}@1.0.0")]);
    let events = EVENTS.lock().unwrap();
    let canceled = events.iter().any(|event| {
        matches!(
            event,
            LogEvent::Global(GlobalLog { level: LogLevel::Info, message }) if message == "Update canceled",
        )
    });
    assert!(canceled, "no `Update canceled` was reported: {events:?}");
}

/// Ports `global interactive update handles an empty global directory`.
#[tokio::test]
async fn global_interactive_update_handles_an_empty_global_directory() {
    let dir = tempfile::tempdir().expect("create temporary global dir");
    let mut config = Config::new();
    config.global_pkg_dir = Some(dir.path().to_path_buf());
    let config = Config::leak(config);
    let scripted = scripted_prompts();

    let selected = super::select_global_package_groups::<pnpm_reporter::SilentReporter>(
        config,
        &[],
        true,
        UpdatePrompt::Scripted,
    )
    .await
    .expect("select global package groups");

    assert!(selected.is_none());
    assert!(scripted.seen().is_empty(), "an empty global directory must not prompt");
}

/// Ports `interactive recursive should not error on git specifier
/// override` (<https://github.com/pnpm/pnpm/issues/7415>). The override
/// upstream sets leaves the lockfile recording a resolution that names no
/// version, which is what the fixture below stands in for.
#[tokio::test]
async fn choices_walk_past_a_dependency_overridden_to_a_git_specifier() {
    let temp = tempfile::tempdir().expect("create temporary workspace");
    let manifest =
        manifest_with_dependency_spec(temp.path(), "project-1", ("is-negative", "2.1.0"));
    let lockfile: Lockfile = serde_saphyr::from_str(
        r"
lockfileVersion: '9.0'
importers:
  project-1:
    dependencies:
      is-negative:
        specifier: 2.1.0
        version: https://codeload.github.com/kevva/is-negative/tar.gz/2.1.0
",
    )
    .expect("parse workspace lockfile");
    let mut config = Config::new();
    // Any request would be a bug: a resolution that names no version has
    // nothing to compare a registry version against.
    config.registry = "http://127.0.0.1:1/".to_string();
    let projects =
        [InteractiveUpdateProject { manifest: &manifest, importer_id: "project-1".to_string() }];

    let choices = collect_choices(
        &projects,
        Some(&lockfile),
        &config,
        &Arc::new(ThrottledClient::default()),
        true,
        &[DependencyGroup::Prod],
    )
    .await
    .expect("collect interactive choices");

    assert!(choices.is_empty());
}
