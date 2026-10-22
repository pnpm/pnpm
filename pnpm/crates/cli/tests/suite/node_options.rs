use assert_cmd::prelude::*;
use pnpm_testing_utils::bin::CommandTempCwd;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::{fs, path::Path, process::Command};

const COMMANDS: [&[&str]; 2] = [&["exec", "node", "probe.cjs"], &["run", "probe"]];
const EMPTY_OVERRIDES: [(Option<&str>, Option<&str>); 4] = [
    (Some("--config.node-options="), None),
    (Some("--config.nodeOptions="), None),
    (None, Some("PNPM_CONFIG_NODE_OPTIONS")),
    (None, Some("pnpm_config_node_options")),
];

fn node_options_project(yaml: &str) -> CommandTempCwd<()> {
    let mut project = CommandTempCwd::init();
    project.pacquet
        .env_remove("NODE_OPTIONS")
        .env_remove("PNPM_CONFIG_NODE_OPTIONS")
        .env_remove("pnpm_config_node_options");
    fs::write(project.workspace.join("pnpm-workspace.yaml"), yaml)
        .expect("write pnpm-workspace.yaml");
    fs::write(
        project.workspace.join("package.json"),
        json!({"name": "node-options-probe", "scripts": {"probe": "node probe.cjs"}}).to_string(),
    )
    .expect("write package.json");
    for source in ["configured", "replacement", "inherited", "hook"] {
        fs::write(
            project.workspace.join(format!("{source}.cjs")),
            format!("globalThis.nodeOptionsSource = '{source}'"),
        )
        .expect("write Node preload");
    }
    fs::write(
        project.workspace.join("probe.cjs"),
        "require('fs').writeFileSync('probe.json', JSON.stringify({ source: globalThis.nodeOptionsSource ?? null, nodeOptions: process.env.NODE_OPTIONS ?? null }))",
    )
    .expect("write Node probe");
    project
}

fn assert_probe(
    command: &mut Command,
    workspace: &Path,
    args: &[&str],
    source: Option<&str>,
    node_options: Option<&str>,
) {
    command.args(args).assert().success();
    let probe: serde_json::Value =
        serde_json::from_slice(&fs::read(workspace.join("probe.json")).expect("read Node probe"))
            .expect("parse Node probe");
    dbg!(&probe);
    assert_eq!(probe, json!({"source": source, "nodeOptions": node_options}), "{args:?}");
}

#[test]
fn node_options_apply_workspace_and_nonempty_overrides() {
    for args in COMMANDS {
        for (flag, env_name, expected) in [
            (None, None, "configured"),
            (Some("--config.node-options=--require=./replacement.cjs"), None, "replacement"),
            (None, Some("PNPM_CONFIG_NODE_OPTIONS"), "replacement"),
        ] {
            let CommandTempCwd { mut pacquet, root, workspace, .. } =
                node_options_project("nodeOptions: --require=./configured.cjs\n");
            pacquet.env("NODE_OPTIONS", "--require=./inherited.cjs");
            if let Some(flag) = flag {
                pacquet.arg(flag);
            }
            if let Some(env_name) = env_name {
                pacquet.env(env_name, "--require=./replacement.cjs");
            }

            assert_probe(
                &mut pacquet,
                &workspace,
                args,
                Some(expected),
                Some(&format!("--require=./{expected}.cjs")),
            );
            drop(root);
        }
    }
}

#[test]
fn empty_node_options_overrides_clear_workspace_options_and_preserve_inherited_options() {
    for args in COMMANDS {
        for (flag, env_name) in EMPTY_OVERRIDES {
            let CommandTempCwd { mut pacquet, root, workspace, .. } =
                node_options_project("nodeOptions: --require=./configured.cjs\n");
            pacquet.env("NODE_OPTIONS", "--require=./inherited.cjs");
            if let Some(flag) = flag {
                pacquet.arg(flag);
            }
            if let Some(env_name) = env_name {
                pacquet.env(env_name, "");
            }

            assert_probe(
                &mut pacquet,
                &workspace,
                args,
                Some("inherited"),
                Some("--require=./inherited.cjs"),
            );
            drop(root);
        }
    }
}

#[test]
fn absent_and_empty_yaml_node_options_preserve_the_parent_environment() {
    for args in COMMANDS {
        for (yaml, inherited) in [
            ("packages: []\n", None),
            ("packages: []\n", Some("--require=./inherited.cjs")),
            ("nodeOptions: ''\n", None),
            ("nodeOptions: ''\n", Some("--require=./inherited.cjs")),
        ] {
            let CommandTempCwd { mut pacquet, root, workspace, .. } = node_options_project(yaml);
            if let Some(inherited) = inherited {
                pacquet.env("NODE_OPTIONS", inherited);
            }

            assert_probe(&mut pacquet, &workspace, args, inherited.map(|_| "inherited"), inherited);
            drop(root);
        }
    }
}

#[test]
fn empty_node_options_preserve_pnpmfile_extra_env() {
    for args in COMMANDS {
        let CommandTempCwd { mut pacquet, root, workspace, .. } =
            node_options_project("nodeOptions: ''\n");
        fs::write(
            workspace.join(".pnpmfile.cjs"),
            "module.exports = { hooks: { updateConfig(config) { config.extraEnv = { ...config.extraEnv, NODE_OPTIONS: '--require=./hook.cjs' }; return config } } }",
        )
        .expect("write pnpmfile");
        pacquet.env("NODE_OPTIONS", "--require=./inherited.cjs");

        assert_probe(&mut pacquet, &workspace, args, Some("hook"), Some("--require=./hook.cjs"));
        drop(root);
    }
}

#[test]
fn config_get_reports_empty_node_options_overrides() {
    for (flag, env_name) in EMPTY_OVERRIDES {
        let CommandTempCwd { mut pacquet, root, .. } =
            node_options_project("nodeOptions: --require=./configured.cjs\n");
        if let Some(flag) = flag {
            pacquet.arg(flag);
        }
        if let Some(env_name) = env_name {
            pacquet.env(env_name, "");
        }

        pacquet
            .args(["config", "get", "node-options"])
            .assert()
            .success()
            .stdout("\n");
        drop(root);
    }
}
