use super::{
    DialoguerPatchPrompt, PatchCandidate, PatchCandidateSet, PatchError, PatchPrompt, PatchTarget,
    checked_existing_patch_file_path, default_edit_dir_name, reject_non_empty_custom_edit_dir,
    reject_non_empty_edit_dir, render_success, select_patch_target,
    select_patch_target_with_prompt,
};
use std::{io::IsTerminal, path::Path};
use tempfile::tempdir;

#[test]
fn select_patch_target_uses_default_when_only_one_candidate_matches() {
    let candidate = prompt_candidate("chalk@5.3.0");
    let set = PatchCandidateSet {
        alias: "chalk".to_string(),
        requested: "chalk@5.3.0".to_string(),
        bare_specifier: Some("5.3.0".to_string()),
        versions: vec![candidate.clone()],
        preferred_versions: vec![candidate],
    };

    let target = select_patch_target(&set).expect("select target");

    assert_eq!(target.alias, "chalk");
    assert_eq!(target.version, "5.3.0");
    assert_eq!(target.bare_specifier, "5.3.0");
    assert!(!target.apply_to_all);
}

#[test]
fn prompt_selection_without_apply_to_all_selects_exact_target() {
    let set = prompt_candidate_set();
    let target =
        select_patch_target_with_prompt(&set, &FakePrompt { selected: 1, apply_to_all: false })
            .expect("select target");

    assert_eq!(target.version, "5.3.0");
    assert_eq!(target.bare_specifier, "5.3.0");
    assert!(!target.apply_to_all);
}

#[test]
fn prompt_selection_with_apply_to_all_selects_name_target() {
    let set = prompt_candidate_set();
    let target =
        select_patch_target_with_prompt(&set, &FakePrompt { selected: 1, apply_to_all: true })
            .expect("select target");

    assert_eq!(target.version, "5.3.0");
    assert_eq!(target.bare_specifier, "5.3.0");
    assert!(target.apply_to_all);
}

#[test]
fn prompt_selection_uses_git_tarball_url_as_bare_specifier() {
    let tarball = "https://codeload.github.com/example/hi/tar.gz/deadbeef";
    let mut set = prompt_candidate_set();
    set.preferred_versions[1].git_tarball_url = Some(tarball.to_string());
    let target =
        select_patch_target_with_prompt(&set, &FakePrompt { selected: 1, apply_to_all: false })
            .expect("select target");

    assert_eq!(target.version, "5.3.0");
    assert_eq!(target.bare_specifier, tarball);
    assert_eq!(target.git_tarball_url.as_deref(), Some(tarball));
    assert!(!target.apply_to_all);
}

#[test]
fn select_patch_target_uses_dialoguer_when_no_default_target() {
    assert!(!std::io::stdin().is_terminal(), "test requires non-interactive stdin");

    assert!(matches!(select_patch_target(&prompt_candidate_set()), Err(PatchError::Canceled)));
}

#[test]
fn dialoguer_prompt_reports_cancellation_when_stdin_is_not_interactive() {
    assert!(!std::io::stdin().is_terminal(), "test requires non-interactive stdin");

    let prompt = DialoguerPatchPrompt;
    let mut git_candidate = prompt_candidate("chalk@5.3.0");
    git_candidate.git_tarball_url =
        Some("https://codeload.github.com/example/chalk/tar.gz/deadbeef".to_string());

    assert!(matches!(
        prompt.select_version(&[git_candidate, prompt_candidate("chalk@4.1.2")]),
        Err(PatchError::Canceled),
    ));
    assert!(matches!(prompt.confirm_apply_to_all(), Err(PatchError::Canceled)));
}

#[test]
fn success_message_colors_edit_dir_and_commit_command_when_enabled() {
    let edit_dir = Path::new("/tmp/edit-dir");
    let rendered = render_success(edit_dir, true);
    let quote = if cfg!(windows) { r#"""# } else { "'" };

    assert!(rendered.contains("\u{1b}[34m/tmp/edit-dir\u{1b}[39m"), "{rendered:?}");
    assert!(
        rendered.contains(&format!(
            "\u{1b}[32mpacquet patch-commit {quote}/tmp/edit-dir{quote}\u{1b}[39m",
        )),
        "{rendered:?}",
    );
}

#[test]
fn success_message_is_plain_when_colors_are_disabled() {
    let edit_dir = Path::new("/tmp/edit-dir");
    let rendered = render_success(edit_dir, false);
    let quote = if cfg!(windows) { r#"""# } else { "'" };

    assert_eq!(
        rendered,
        format!(
            "Patch: You can now edit the package at:\n\n  /tmp/edit-dir\n\nTo commit your changes, run:\n\n  pacquet patch-commit {quote}/tmp/edit-dir{quote}\n\n",
        ),
    );
}

#[cfg(unix)]
#[test]
fn success_message_shell_quotes_single_quotes_in_edit_dir() {
    let edit_dir = Path::new("/tmp/patch user's dir");
    let rendered = render_success(edit_dir, false);

    assert!(rendered.contains(r"pacquet patch-commit '/tmp/patch user'\''s dir'"), "{rendered}");
}

#[cfg(unix)]
#[test]
fn existing_patch_file_path_rejects_non_regular_files() {
    let tmp = tempdir().expect("temp dir");
    let patches_dir = tmp.path().join("patches");
    std::fs::create_dir(&patches_dir).expect("create patches dir");
    let socket_path = patches_dir.join("pkg.patch");
    let _listener =
        std::os::unix::net::UnixListener::bind(&socket_path).expect("create patch socket");

    let err = checked_existing_patch_file_path(tmp.path(), "patches", "patches/pkg.patch")
        .expect_err("socket patch path should be rejected");

    assert!(matches!(err, PatchError::PatchFileNotRegular { .. }));
}

#[test]
fn edit_dir_rejectors_accept_missing_and_empty_dirs() {
    let tmp = tempdir().expect("temp dir");
    let missing = tmp.path().join("missing");
    let empty = tmp.path().join("empty");
    std::fs::create_dir(&empty).expect("create empty dir");

    reject_non_empty_custom_edit_dir(&missing).expect("missing custom edit dir");
    reject_non_empty_custom_edit_dir(&empty).expect("empty custom edit dir");
    reject_non_empty_edit_dir(&missing).expect("missing edit dir");
    reject_non_empty_edit_dir(&empty).expect("empty edit dir");
}

#[test]
fn edit_dir_rejectors_reject_non_empty_dirs_with_command_specific_errors() {
    let tmp = tempdir().expect("temp dir");
    let edit_dir = tmp.path().join("edit");
    std::fs::create_dir(&edit_dir).expect("create edit dir");
    std::fs::write(edit_dir.join("index.js"), "module.exports = true\n").expect("write file");

    assert!(matches!(
        reject_non_empty_custom_edit_dir(&edit_dir),
        Err(PatchError::PatchEditDirExists { .. }),
    ));
    assert!(matches!(
        reject_non_empty_edit_dir(&edit_dir),
        Err(PatchError::EditDirNotEmpty { .. })
    ));
}

#[cfg(unix)]
#[test]
fn custom_edit_dir_rejector_rejects_symlinked_edit_dir() {
    let tmp = tempdir().expect("temp dir");
    let outside = tmp.path().join("outside");
    let edit_dir = tmp.path().join("edit");
    std::fs::create_dir(&outside).expect("create outside dir");
    std::os::unix::fs::symlink(&outside, &edit_dir).expect("symlink edit dir");

    let err = reject_non_empty_custom_edit_dir(&edit_dir)
        .expect_err("custom edit dir symlink should be rejected");

    assert!(matches!(err, PatchError::EditDirSymlink { .. }));
}

#[test]
fn default_edit_dir_name_sanitizes_bare_specifier_path_chars() {
    let target = PatchTarget {
        alias: "@scope/pkg".to_string(),
        version: "1.0.0".to_string(),
        bare_specifier: "npm:@scope/pkg@^1.0.0".to_string(),
        apply_to_all: false,
        git_tarball_url: None,
        package_key: "@scope/pkg@1.0.0".parse().expect("package key"),
    };

    assert_eq!(
        default_edit_dir_name("@scope/pkg@npm:@scope/pkg@^1.0.0", &target),
        "@scope/pkg@npm+@scope+pkg@^1.0.0",
    );
}

#[test]
fn default_edit_dir_name_falls_back_to_alias_then_requested_package() {
    let mut target = PatchTarget {
        alias: "chalk".to_string(),
        version: "5.3.0".to_string(),
        bare_specifier: String::new(),
        apply_to_all: false,
        git_tarball_url: None,
        package_key: "chalk@5.3.0".parse().expect("package key"),
    };

    assert_eq!(default_edit_dir_name("chalk", &target), "chalk");

    target.alias.clear();
    assert_eq!(default_edit_dir_name("chalk@npm:chalk@5.3.0", &target), "chalk@npm:chalk@5.3.0");
}

struct FakePrompt {
    selected: usize,
    apply_to_all: bool,
}

impl PatchPrompt for FakePrompt {
    fn select_version(&self, _candidates: &[PatchCandidate]) -> Result<usize, PatchError> {
        Ok(self.selected)
    }

    fn confirm_apply_to_all(&self) -> Result<bool, PatchError> {
        Ok(self.apply_to_all)
    }
}

fn prompt_candidate_set() -> PatchCandidateSet {
    let candidates = vec![prompt_candidate("chalk@4.1.2"), prompt_candidate("chalk@5.3.0")];
    PatchCandidateSet {
        alias: "chalk".to_string(),
        requested: "chalk".to_string(),
        bare_specifier: None,
        versions: candidates.clone(),
        preferred_versions: candidates,
    }
}

fn prompt_candidate(key: &str) -> PatchCandidate {
    let package_key = key.parse().expect("package key");
    let version = key.rsplit('@').next().expect("version").to_string();
    PatchCandidate { name: "chalk".to_string(), version, git_tarball_url: None, package_key }
}
