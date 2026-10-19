//! Every writer of an injected copy leaves the files the install selects:
//! the package's packlist, or every file under `deployAllFiles`
//! (pnpm/tasks#145).
//!
//! The writers, in the order a scenario runs them:
//!
//! 1. the install, which also refreshes the copy after the package's
//!    `prepare` script;
//! 2. the sync after a script listed in `syncInjectedDepsAfterScripts`;
//! 3. the same sync after a source edit;
//! 4. a frozen install into a clean tree.

use crate::_utils::{WorkspaceFixture, assert_success, copy_sources, walk_files};

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde_json::{Value, json};

#[derive(Debug, Clone, Copy)]
struct Layout {
    shared_lockfile: bool,
    hoisted: bool,
    deploy_all_files: bool,
}

/// `writer.cjs` writes `dist/<name>.js`, which `files` lists, and
/// `<name>.log`, which it leaves out.
const WRITER: &str = "const fs = require('fs')\n\
const name = process.argv[2]\n\
fs.mkdirSync(__dirname + '/dist', { recursive: true })\n\
fs.writeFileSync(__dirname + '/dist/' + name + '.js', fs.readFileSync(__dirname + '/index.js'))\n\
fs.writeFileSync(__dirname + '/' + name + '.log', name)\n";

struct Scenario {
    fixture: WorkspaceFixture,
    layout: Layout,
}

impl Scenario {
    /// `app` injects `lib`. `lib` lists `dist` and `index.js` in `files`,
    /// and also holds a README that every packlist includes, a source file
    /// and a secret that `files` leaves out.
    fn new(layout: Layout) -> Self {
        let fixture = WorkspaceFixture::new();
        fixture.append_workspace_yaml(&format!(
            "sharedWorkspaceLockfile: {}\nnodeLinker: {}\ndeployAllFiles: {}\n\
             dedupeInjectedDeps: false\nsyncInjectedDepsAfterScripts:\n  - build\n",
            layout.shared_lockfile,
            if layout.hoisted { "hoisted" } else { "isolated" },
            layout.deploy_all_files,
        ));
        let scenario = Self { fixture, layout };
        scenario.write_manifest("root", "", json!({}));
        scenario.write_manifest(
            "app",
            "packages/app",
            json!({
                "dependencies": { "lib": "workspace:*" },
                "dependenciesMeta": { "lib": { "injected": true } },
            }),
        );
        scenario.write_manifest(
            "lib",
            "packages/lib",
            json!({
                "files": ["dist", "index.js"],
                "scripts": {
                    "prepare": "node writer.cjs prepared",
                    "build": "node writer.cjs built",
                },
            }),
        );
        let lib = scenario.lib();
        for (file, contents) in [
            ("writer.cjs", WRITER),
            ("index.js", "module.exports = 1\n"),
            ("README.md", "# lib\n"),
            ("secret.txt", "secret\n"),
            ("src/source.ts", "export default 1\n"),
        ] {
            let path = lib.join(file);
            fs::create_dir_all(path.parent().expect("a parent")).expect("create a directory");
            fs::write(path, contents).expect("write a lib file");
        }
        scenario
    }

    fn workspace(&self) -> &Path {
        &self.fixture.workspace
    }

    fn lib(&self) -> PathBuf {
        self.workspace().join("packages/lib")
    }

    fn write_manifest(&self, name: &str, dir: &str, fields: Value) {
        let dir = self.workspace().join(dir);
        fs::create_dir_all(&dir).expect("create the project dir");
        let mut manifest = json!({ "name": name, "version": "1.0.0", "private": true });
        let Value::Object(fields) = fields else { panic!("the fields must be an object") };
        manifest
            .as_object_mut()
            .expect("a manifest object")
            .extend(fields);
        fs::write(dir.join("package.json"), manifest.to_string()).expect("write package.json");
    }

    fn run(&self, cwd: &Path, args: &[&str]) {
        assert_success(&self.fixture.command_at(cwd, args));
    }

    /// The files the install selects from `lib` in `workspace`, by path,
    /// with their contents.
    fn expected_files(&self, workspace: &Path) -> BTreeMap<String, Vec<u8>> {
        let lib = workspace.join("packages/lib");
        let paths: Vec<String> = if self.layout.deploy_all_files {
            files_under(&lib).into_keys().collect()
        } else {
            self.packlist(&lib)
        };
        paths
            .into_iter()
            .map(|path| {
                let contents = fs::read(lib.join(&path)).expect("read a packed file");
                (path, contents)
            })
            .collect()
    }

    /// `pnpm pack --dry-run` of a copy of `lib`, so no script touches it.
    fn packlist(&self, lib: &Path) -> Vec<String> {
        let copy = self.workspace().with_file_name("packlist-source");
        let _ = fs::remove_dir_all(&copy);
        copy_sources(lib, &copy);
        let output = self.fixture.command_at(
            &copy,
            ["pack", "--dry-run", "--json", "--ignore-scripts", "--reporter=silent"],
        );
        assert_success(&output);
        let packed: Value = serde_json::from_slice(&output.stdout).expect("parse pack --json");
        packed["files"]
            .as_array()
            .expect("a files list")
            .iter()
            .map(|file| {
                file["path"]
                    .as_str()
                    .expect("a path")
                    .to_string()
            })
            .collect()
    }

    /// The injected copy that `app` in `workspace` resolves `lib` to.
    fn injected_copy(&self, workspace: &Path) -> PathBuf {
        let app = workspace.join("packages/app");
        let found = app
            .ancestors()
            .map(|dir| dir.join("node_modules/lib"))
            .find(|candidate| candidate.join("package.json").is_file())
            .unwrap_or_else(|| panic!("{:?}: app does not resolve lib", self.layout));
        let copy = fs::canonicalize(found).expect("canonicalize the copy");
        let source = fs::canonicalize(workspace.join("packages/lib")).expect("canonicalize lib");
        assert_ne!(copy, source, "{:?}: lib is linked, not injected", self.layout);
        copy
    }

    #[track_caller]
    fn assert_copy(&self, workspace: &Path, writer: &str) {
        let actual = files_under(&self.injected_copy(workspace));
        let expected = self.expected_files(workspace);
        assert_eq!(
            actual.keys().collect::<Vec<_>>(),
            expected.keys().collect::<Vec<_>>(),
            "{:?}, after {writer}: the injected copy holds other files than the install selects",
            self.layout,
        );
        let differing: Vec<(&String, String, String)> = actual
            .iter()
            .filter(|(path, contents)| expected.get(*path) != Some(*contents))
            .map(|(path, contents)| {
                (
                    path,
                    String::from_utf8_lossy(contents).into_owned(),
                    String::from_utf8_lossy(&expected[path]).into_owned(),
                )
            })
            .collect();
        assert!(
            differing.is_empty(),
            "{:?}, after {writer}: files of the injected copy differ from the source \
             (path, copy, source): {differing:#?}",
            self.layout,
        );
    }
}

/// Every file under `dir` outside `node_modules`, by its `/`-separated path.
fn files_under(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    walk_files(dir)
        .into_iter()
        .filter_map(|path| {
            let relative = path.strip_prefix(dir).expect("under the directory");
            if relative
                .components()
                .any(|part| part.as_os_str() == "node_modules")
            {
                return None;
            }
            let contents = fs::read(&path).expect("read a file");
            Some((relative.to_string_lossy().replace('\\', "/"), contents))
        })
        .collect()
}

fn check(layout: Layout) {
    let scenario = Scenario::new(layout);
    let workspace = scenario.workspace().to_path_buf();
    let lib = scenario.lib();

    scenario.run(&workspace, &["install"]);
    assert!(lib.join("dist/prepared.js").is_file(), "the install ran lib's prepare");
    scenario.assert_copy(&workspace, "the install");

    scenario.run(&lib, &["run", "build"]);
    scenario.assert_copy(&workspace, "a listed script");

    fs::write(lib.join("index.js"), "module.exports = 2\n").expect("edit index.js");
    scenario.run(&lib, &["run", "build"]);
    scenario.assert_copy(&workspace, "a listed script after an edit");

    let clean = workspace.with_file_name("clean");
    copy_sources(&workspace, &clean);
    scenario.run(&clean, &["install", "--frozen-lockfile"]);
    scenario.assert_copy(&clean, "a frozen install into a clean tree");
}

macro_rules! matrix {
    ($($name:ident: $layout:expr,)*) => {
        $(
            #[test]
            fn $name() {
                check($layout);
            }
        )*
    };
}

matrix! {
    shared_isolated: Layout { shared_lockfile: true, hoisted: false, deploy_all_files: false },
    shared_hoisted: Layout { shared_lockfile: true, hoisted: true, deploy_all_files: false },
    dedicated_isolated: Layout { shared_lockfile: false, hoisted: false, deploy_all_files: false },
    dedicated_hoisted: Layout { shared_lockfile: false, hoisted: true, deploy_all_files: false },
    shared_isolated_all_files: Layout { shared_lockfile: true, hoisted: false, deploy_all_files: true },
    shared_hoisted_all_files: Layout { shared_lockfile: true, hoisted: true, deploy_all_files: true },
    dedicated_isolated_all_files: Layout {
        shared_lockfile: false,
        hoisted: false,
        deploy_all_files: true,
    },
    dedicated_hoisted_all_files: Layout {
        shared_lockfile: false,
        hoisted: true,
        deploy_all_files: true,
    },
}
