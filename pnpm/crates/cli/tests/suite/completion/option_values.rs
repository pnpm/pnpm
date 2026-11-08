use super::{pacquet, prepend_binary_dir_to_path, stdout};
use std::process::Command;
use tempfile::TempDir;

#[test]
fn completion_server_completes_workspace_filter_values() {
    let workspace = TempDir::new().unwrap();
    std::fs::write(workspace.path().join("package.json"), r#"{"name":"root"}"#).unwrap();
    std::fs::write(
        workspace.path().join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\n  - '!packages/excluded'\n",
    )
    .unwrap();
    for (directory, manifest) in [
        ("foo", r#"{"name":"pkg-foo"}"#),
        ("bar", r#"{"name":"@scope/bar"}"#),
        ("unnamed", "{}"),
        ("excluded", r#"{"name":"excluded"}"#),
    ] {
        let directory = workspace
            .path()
            .join("packages")
            .join(directory);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("package.json"), manifest).unwrap();
    }
    for words in [
        vec!["pnpm", "--filter", ""],
        vec!["pnpm", "-F", ""],
        vec!["pnpm", "-rF", ""],
        vec!["pnpm", "--filter-prod", ""],
        vec!["pnpm", "run", "--filter", ""],
        vec!["pnpm", "--filter", "pkg-foo", "-F", ""],
    ] {
        let output = pacquet()
            .current_dir(workspace.path())
            .args(["completion-server", "--"])
            .args(&words)
            .output()
            .unwrap();
        assert_eq!(stdout(output), "@scope/bar\npkg-foo\nroot\n", "{words:?}");
    }
    for (prefix, expected) in [("pkg", "pkg-foo\n"), ("@scope/", "@scope/bar\n"), ("missing", "")] {
        let output = pacquet()
            .current_dir(workspace.path().join("packages/foo"))
            .args(["completion-server", "--", "pnpm", "-F", prefix])
            .output()
            .unwrap();
        assert_eq!(stdout(output), expected, "{prefix}");
    }
    for (word, expected) in [
        ("--filter=pkg", "--filter=pkg-foo\n"),
        ("--filter-prod=pkg", "--filter-prod=pkg-foo\n"),
        ("-F=pkg", "-F=pkg-foo\n"),
        ("-Fpkg", "-Fpkg-foo\n"),
        ("-rFpkg", "-rFpkg-foo\n"),
    ] {
        let output = pacquet()
            .current_dir(workspace.path())
            .args(["completion-server", "--", "pnpm", word])
            .output()
            .unwrap();
        assert_eq!(stdout(output), expected, "{word}");
    }
    for directory_option in
        [vec!["--dir", "packages/foo"], vec!["-Cpackages/foo"], vec!["--prefix=packages/foo"]]
    {
        let output = pacquet()
            .current_dir(workspace.path())
            .args(["completion-server", "--", "pnpm"])
            .args(&directory_option)
            .args(["--filter", "pkg"])
            .output()
            .unwrap();
        assert_eq!(stdout(output), "pkg-foo\n", "{directory_option:?}");
    }
}

#[test]
fn completion_server_filter_values_without_workspace() {
    for (manifest, expected) in
        [(None, ""), (Some("{}"), ""), (Some(r#"{"name":"standalone"}"#), "standalone\n")]
    {
        let project = TempDir::new().unwrap();
        if let Some(manifest) = manifest {
            std::fs::write(project.path().join("package.json"), manifest).unwrap();
        }
        let output = pacquet()
            .current_dir(project.path())
            .args(["completion-server", "--", "pnpm", "--filter", ""])
            .output()
            .unwrap();
        assert_eq!(stdout(output), expected);
    }
}

#[test]
fn completion_server_filter_respects_root_only_workspaces() {
    for config in ["packages: []\n", "linkWorkspacePackages: true\n"] {
        let workspace = TempDir::new().unwrap();
        let child = workspace.path().join("child");
        std::fs::create_dir(&child).unwrap();
        std::fs::write(workspace.path().join("package.json"), r#"{"name":"root"}"#).unwrap();
        std::fs::write(child.join("package.json"), r#"{"name":"child"}"#).unwrap();
        std::fs::write(workspace.path().join("pnpm-workspace.yaml"), config).unwrap();
        let output = pacquet()
            .current_dir(workspace.path())
            .args(["completion-server", "--", "pnpm", "--filter", ""])
            .output()
            .unwrap();
        assert_eq!(stdout(output), "root\n", "{config}");
    }
}

#[test]
fn completion_server_omits_unsafe_package_and_script_names() {
    let workspace = TempDir::new().unwrap();
    let unsafe_names = [
        "bad\nname",
        "bad\rname",
        "bad\tname",
        "bad\u{1b}[31mname",
        "bad\u{7f}name",
        "bad\u{85}name",
        "bad\u{202e}name",
        "bad\u{2028}name",
    ];
    let scripts: serde_json::Map<_, _> = std::iter::once("safe")
        .chain(unsafe_names)
        .map(|name| (name.to_string(), serde_json::json!("echo unused")))
        .collect();
    std::fs::write(
        workspace.path().join("package.json"),
        serde_json::to_vec(&serde_json::json!({"name": "safe", "scripts": scripts})).unwrap(),
    )
    .unwrap();
    std::fs::write(workspace.path().join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .unwrap();
    for (index, name) in unsafe_names.iter().enumerate() {
        let directory = workspace
            .path()
            .join(format!("packages/pkg-{index}"));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("package.json"),
            serde_json::to_vec(&serde_json::json!({"name": name})).unwrap(),
        )
        .unwrap();
    }
    for shell in ["bash", "fish", "pwsh", "zsh"] {
        for option in ["--filter", "-F", "run"] {
            let output = pacquet()
                .current_dir(workspace.path())
                .env("SHELL", shell)
                .args(["completion-server", "--", "pnpm", option, ""])
                .output()
                .unwrap();
            assert_eq!(stdout(output), "safe\n", "{shell}: {option}");
        }
    }
}

#[test]
fn completion_server_completes_directory_option_values() {
    let workspace = TempDir::new().unwrap();
    std::fs::create_dir_all(workspace.path().join("alpha/nested")).unwrap();
    std::fs::create_dir(workspace.path().join("beta")).unwrap();
    std::fs::create_dir(workspace.path().join(".hidden-dir")).unwrap();
    std::fs::write(workspace.path().join("gamma.txt"), "not a directory").unwrap();

    for words in [
        vec!["pnpm", "-C", ""],
        vec!["pnpm", "--dir", ""],
        vec!["pnpm", "--prefix", ""],
        vec!["pnpm", "--store-dir", ""],
        vec!["pnpm", "--store", ""],
        vec!["pnpm", "--state-dir", ""],
        vec!["pnpm", "-rC", ""],
        vec!["pnpm", "install", "-C", ""],
        vec!["pnpm", "install", "--lockfile-dir", ""],
        vec!["pnpm", "patch", "pkg", "-d", ""],
        vec!["pnpm", "patch", "pkg", "--edit-dir", ""],
        vec!["pnpm", "patch-commit", "dir", "--patches-dir", ""],
        vec!["pnpm", "pack", "--pack-destination", ""],
        vec!["pnpm", "pack-app", "-o", ""],
        vec!["pnpm", "pack-app", "--output-dir", ""],
    ] {
        let output = pacquet()
            .current_dir(workspace.path())
            .args(["completion-server", "--"])
            .args(&words)
            .output()
            .unwrap();
        assert_eq!(stdout(output), "alpha/\nbeta/\n", "{words:?}");
    }

    for (words, expected) in [
        (vec!["pnpm", "-C", "./"], "./alpha/\n./beta/\n"),
        (vec!["pnpm", "--dir", "./al"], "./alpha/\n"),
        (vec!["pnpm", "-C", "alpha/"], "alpha/nested/\n"),
        (vec!["pnpm", "-C", "."], ".hidden-dir/\n"),
        (vec!["pnpm", "--dir="], "--dir=alpha/\n--dir=beta/\n"),
        (vec!["pnpm", "--dir=./"], "--dir=./alpha/\n--dir=./beta/\n"),
        (vec!["pnpm", "--dir=al"], "--dir=alpha/\n"),
        (vec!["pnpm", "--prefix=al"], "--prefix=alpha/\n"),
        (vec!["pnpm", "-C=al"], "-C=alpha/\n"),
        (vec!["pnpm", "-C./"], "-C./alpha/\n-C./beta/\n"),
        (vec!["pnpm", "-Cal"], "-Calpha/\n"),
        (vec!["pnpm", "-rC./"], "-rC./alpha/\n-rC./beta/\n"),
        (vec!["pnpm", "-C", "alpha", "install", "--lockfile-dir", ""], "nested/\n"),
        (vec!["pnpm", "--dir=alpha", "install", "--lockfile-dir="], "--lockfile-dir=nested/\n"),
        (vec!["pnpm", "-C", "alpha", "--dir", ""], "alpha/\nbeta/\n"),
    ] {
        let output = pacquet()
            .current_dir(workspace.path())
            .args(["completion-server", "--"])
            .args(&words)
            .output()
            .unwrap();
        assert_eq!(stdout(output), expected, "{words:?}");
    }
}

#[test]
fn completion_server_completes_file_option_values() {
    let workspace = TempDir::new().unwrap();
    std::fs::create_dir(workspace.path().join("configs")).unwrap();
    std::fs::write(workspace.path().join("gamma.txt"), "").unwrap();
    std::fs::write(workspace.path().join(".env"), "").unwrap();

    for words in [
        vec!["pnpm", "--env-file", ""],
        vec!["pnpm", "--npmrc-auth-file", ""],
        vec!["pnpm", "--userconfig", ""],
        vec!["pnpm", "pack", "--out", ""],
        vec!["pnpm", "pack-app", "--entry", ""],
        vec!["pnpm", "sbom", "--out", ""],
        vec!["pnpm", "publish", "--release-notes", ""],
        vec!["pnpm", "version", "--release-notes", ""],
    ] {
        let output = pacquet()
            .current_dir(workspace.path())
            .args(["completion-server", "--"])
            .args(&words)
            .output()
            .unwrap();
        assert_eq!(stdout(output), "configs/\ngamma.txt\n", "{words:?}");
    }

    for (words, expected) in [
        (vec!["pnpm", "--env-file", "."], ".env\n"),
        (vec!["pnpm", "--env-file=."], "--env-file=.env\n"),
        (vec!["pnpm", "--env-file=ga"], "--env-file=gamma.txt\n"),
    ] {
        let output = pacquet()
            .current_dir(workspace.path())
            .args(["completion-server", "--"])
            .args(&words)
            .output()
            .unwrap();
        assert_eq!(stdout(output), expected, "{words:?}");
    }
}

#[cfg_attr(not(unix), ignore = "requires Bash")]
#[test]
fn completion_bash_completes_directory_options() {
    let workspace = TempDir::new().unwrap();
    std::fs::create_dir(workspace.path().join("alpha")).unwrap();
    std::fs::create_dir(workspace.path().join("beta")).unwrap();
    std::fs::write(workspace.path().join("gamma.txt"), "").unwrap();

    let completion_script = stdout(
        pacquet()
            .args(["completion", "bash"])
            .output()
            .unwrap(),
    );
    for (words, line, expected) in [
        ("(pnpm -C '')", "pnpm -C ", &["alpha/", "beta/"][..]),
        ("(pnpm --dir = al)", "pnpm --dir=al", &["alpha/"][..]),
        ("(pnpm -C./al)", "pnpm -C./al", &["-C./alpha/"][..]),
    ] {
        let bash_script = format!(
            r#"{completion_script}
COMP_WORDS={words}
COMP_CWORD=$((${{#COMP_WORDS[@]}} - 1))
COMP_LINE='{line}'
COMP_POINT=${{#COMP_LINE}}
_pnpm_completion
printf '%s\n' "${{COMPREPLY[@]}}"
"#,
        );
        let output = Command::new("bash")
            .current_dir(workspace.path())
            .env("PATH", prepend_binary_dir_to_path())
            .args(["--noprofile", "--norc", "-c", &bash_script])
            .output()
            .unwrap();
        let reply = stdout(output);
        assert_eq!(reply.lines().collect::<Vec<_>>(), expected, "{line}");
    }
}
