use super::{fs, json, packlist, tempdir, touch, write};
use proptest::{
    arbitrary::any,
    collection, option, proptest,
    sample::select,
    strategy::{Just, Strategy},
    test_runner::Config as ProptestConfig,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::Mutex,
};

/// Deliberate, documented behavioral differences between `pnpm-fs-packlist`
/// and upstream `npm-packlist` / `@pnpm/fs.packlist`.
///
/// Every difference observed by [`generated_trees_match_npm_packlist`] or
/// [`pack_matches_npm_pack_dry_run`] must be classified by an entry in
/// [`DELIBERATE_DIVERGENCES`], and [`deliberate_divergences_have_witnesses`]
/// proves each allowlisted variant is exercised.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DeliberateDivergence {
    /// `pnpm-fs-packlist` admits symlinks whose target stays within the
    /// package root; upstream `ignore-walk` drops all symlinks.
    InternalSymlink,
    /// `pnpm-fs-packlist` always includes a root `package.json5` or
    /// `package.yaml`; `@pnpm/fs.packlist` only includes one when
    /// `package.json` is absent, and upstream `npm-packlist` only includes
    /// `package.json`.
    AlternateRootManifest,
    /// `pnpm-fs-packlist` includes the file named by `manifest.pnpm.config`.
    PnpmConfigPath,
    /// `pnpm-fs-packlist` hard-prunes `.git`, `.svn`, `.hg`, and `CVS`
    /// directories at any depth even when `files` or `bin` attempts to
    /// re-include them; `npm-packlist` only puts `/.git` in `strictDefaults`.
    AlwaysExcludedVcsDirectories,
    /// `pnpm/pnpm#16920`: a literal file path in `manifest.files` at depth >= 2
    /// (`src/util/file.js`) survives a subdirectory `.npmignore`/`.gitignore`;
    /// `npm-packlist`'s `relative(file, entry) === '..'` check drops
    /// `requiredFiles` past depth 1.
    NamedFileDepthRule,
    /// `pnpm/pnpm#16918`: `main`, `browser`, and `bin` entry points match their
    /// literal path with exact case (`IgnoreRule::reinclude_path`), whereas
    /// `npm-packlist` compiles them as case-insensitive globs.
    ExactCaseEntryPoint,
    /// `npm` v11 (`npm-packlist` v10) anchors negated `files` entries at the
    /// package root (`!*.map` -> `/*.map`), whereas `npm-packlist` v8,
    /// `@pnpm/fs.packlist`, and `pnpm-fs-packlist` keep them unanchored so
    /// `"!*.map"` excludes nested `lib/*.map` files.
    UnanchoredFilesFieldNegation,
}

pub const DELIBERATE_DIVERGENCES: &[DeliberateDivergence] = &[
    DeliberateDivergence::InternalSymlink,
    DeliberateDivergence::AlternateRootManifest,
    DeliberateDivergence::PnpmConfigPath,
    DeliberateDivergence::AlwaysExcludedVcsDirectories,
    DeliberateDivergence::NamedFileDepthRule,
    DeliberateDivergence::ExactCaseEntryPoint,
    DeliberateDivergence::UnanchoredFilesFieldNegation,
];

#[derive(Debug, Deserialize)]
struct OracleResponse {
    npm: Vec<String>,
    pnpm: Vec<String>,
}

struct PersistentOracle {
    _child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl PersistentOracle {
    fn spawn() -> Self {
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tests/parity.mjs");
        let mut child = Command::new("node")
            .arg(script)
            .arg("--stdio")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn node parity.mjs --stdio");
        let stdin = child.stdin.take().expect("oracle stdin");
        let stdout = BufReader::new(child.stdout.take().expect("oracle stdout"));
        Self { _child: child, stdin, stdout }
    }

    fn query(&mut self, root: &Path, manifest: &Value) -> OracleResponse {
        let line = serde_json::to_string(&json!({ "root": root, "manifest": manifest })).unwrap();
        writeln!(self.stdin, "{line}").expect("write to oracle stdin");
        self.stdin.flush().expect("flush oracle stdin");
        let mut out = String::new();
        let read = self.stdout.read_line(&mut out).expect("read from oracle stdout");
        assert!(read > 0, "oracle exited unexpectedly");
        serde_json::from_str(&out).expect("parse oracle response")
    }
}

fn with_oracle<Output>(f: impl FnOnce(&mut PersistentOracle) -> Output) -> Output {
    static ORACLE: Mutex<Option<PersistentOracle>> = Mutex::new(None);
    let mut guard = ORACLE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let oracle = guard.get_or_insert_with(PersistentOracle::spawn);
    f(oracle)
}

/// Explain why `path` differs between `pnpm-fs-packlist` (`in_rust`) and an
/// oracle (`!in_rust`), returning `None` when the difference is not covered by
/// [`DELIBERATE_DIVERGENCES`].
fn classify_divergence(
    root: &Path,
    manifest: &Value,
    path: &str,
    in_rust: bool,
) -> Option<DeliberateDivergence> {
    let candidate = if in_rust {
        classify_rust_only_path(root, manifest, path)
    } else {
        classify_oracle_only_path(manifest, path)
    };
    candidate.filter(|divergence| DELIBERATE_DIVERGENCES.contains(divergence))
}

fn classify_rust_only_path(
    root: &Path,
    manifest: &Value,
    path: &str,
) -> Option<DeliberateDivergence> {
    if is_or_under_symlink(root, path) {
        return Some(DeliberateDivergence::InternalSymlink);
    }
    if path.eq_ignore_ascii_case("package.json5") || path.eq_ignore_ascii_case("package.yaml") {
        return Some(DeliberateDivergence::AlternateRootManifest);
    }
    if crate::pnpm_config_path(manifest).as_deref() == Some(path) {
        return Some(DeliberateDivergence::PnpmConfigPath);
    }
    if is_named_file_in_manifest(manifest, path) {
        return Some(DeliberateDivergence::NamedFileDepthRule);
    }
    None
}

fn classify_oracle_only_path(manifest: &Value, path: &str) -> Option<DeliberateDivergence> {
    if path
        .split('/')
        .any(|seg| crate::ALWAYS_EXCLUDED_DIR_SEGMENTS.contains(&seg))
    {
        return Some(DeliberateDivergence::AlwaysExcludedVcsDirectories);
    }
    for required in crate::required_field_paths(manifest) {
        if required != path && required.eq_ignore_ascii_case(path) {
            return Some(DeliberateDivergence::ExactCaseEntryPoint);
        }
        if let Some((req_top, _)) = required.split_once('/')
            && let Some((path_top, _)) = path.split_once('/')
            && !req_top.eq(path_top)
            && req_top.eq_ignore_ascii_case(path_top)
        {
            return Some(DeliberateDivergence::ExactCaseEntryPoint);
        }
    }
    if has_unanchored_files_negation(manifest) {
        return Some(DeliberateDivergence::UnanchoredFilesFieldNegation);
    }
    None
}

fn has_unanchored_files_negation(manifest: &Value) -> bool {
    manifest
        .get("files")
        .and_then(Value::as_array)
        .is_some_and(|files| {
            files
                .iter()
                .filter_map(Value::as_str)
                .any(|entry| entry.starts_with('!') && !entry[1..].contains('/'))
        })
}

fn is_or_under_symlink(root: &Path, rel: &str) -> bool {
    let mut cur = root.to_path_buf();
    for part in rel.split('/') {
        cur.push(part);
        if pnpm_fs::is_symlink(&cur) {
            return true;
        }
    }
    false
}

fn is_named_file_in_manifest(manifest: &Value, path: &str) -> bool {
    manifest
        .get("files")
        .and_then(Value::as_array)
        .is_some_and(|files| {
            files
                .iter()
                .filter_map(Value::as_str)
                .any(|raw| {
                    let cleaned = crate::ignore_walk::files_field::normalize_field_path(raw);
                    let trimmed = cleaned.trim_start_matches('/').trim_end_matches('/');
                    trimmed == path
                })
        })
}

fn assert_matches_oracles(root: &Path, manifest: &Value, context: &str) {
    let mut actual = packlist(root, manifest).unwrap();
    actual.sort();
    let OracleResponse { npm, pnpm } = with_oracle(|oracle| oracle.query(root, manifest));

    let actual_set: BTreeSet<&str> = actual
        .iter()
        .map(String::as_str)
        .collect();
    let npm_set: BTreeSet<&str> = npm.iter().map(String::as_str).collect();
    let pnpm_set: BTreeSet<&str> = pnpm
        .iter()
        .map(String::as_str)
        .collect();

    for path in actual_set.difference(&npm_set) {
        assert!(
            classify_divergence(root, manifest, path, true).is_some(),
            "unexpected path in pnpm-fs-packlist but not npm-packlist: {path:?}\ncontext: {context}\nactual: {actual:?}\nnpm: {npm:?}\npnpm_ts: {pnpm:?}",
        );
    }
    for path in npm_set.difference(&actual_set) {
        assert!(
            classify_divergence(root, manifest, path, false).is_some(),
            "unexpected path in npm-packlist but not pnpm-fs-packlist: {path:?}\ncontext: {context}\nactual: {actual:?}\nnpm: {npm:?}\npnpm_ts: {pnpm:?}",
        );
    }
    for path in actual_set.difference(&pnpm_set) {
        assert!(
            classify_divergence(root, manifest, path, true).is_some(),
            "unexpected path in pnpm-fs-packlist but not @pnpm/fs.packlist: {path:?}\ncontext: {context}\nactual: {actual:?}\npnpm_ts: {pnpm:?}",
        );
    }
    for path in pnpm_set.difference(&actual_set) {
        assert!(
            classify_divergence(root, manifest, path, false).is_some(),
            "unexpected path in @pnpm/fs.packlist but not pnpm-fs-packlist: {path:?}\ncontext: {context}\nactual: {actual:?}\npnpm_ts: {pnpm:?}",
        );
    }
}

#[test]
fn deliberate_divergences_have_witnesses() {
    let mut observed = BTreeSet::new();

    // 1. InternalSymlink
    {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write(root, "package.json", r#"{"name":"pkg","version":"1.0.0"}"#);
        touch(root, "real.js");
        pnpm_fs::symlink_file_or_copy(
            Path::new("real.js"),
            &root.join("real.js"),
            &root.join("link.js"),
        )
        .unwrap();
        let manifest = json!({"name":"pkg","version":"1.0.0"});
        let actual = packlist(root, &manifest).unwrap();
        let resp = with_oracle(|o| o.query(root, &manifest));
        if actual.contains(&"link.js".to_string()) && !resp.npm.contains(&"link.js".to_string()) {
            observed.insert(
                classify_divergence(root, &manifest, "link.js", true)
                    .expect("InternalSymlink witness"),
            );
        }
    }

    // 2. AlternateRootManifest
    {
        let dir = tempdir().unwrap();
        let root = dir.path();
        write(root, "package.json", r#"{"name":"pkg","version":"1.0.0","files":["index.js"]}"#);
        write(root, "package.json5", "{}");
        touch(root, "index.js");
        let manifest = json!({"name":"pkg","version":"1.0.0","files":["index.js"]});
        let actual = packlist(root, &manifest).unwrap();
        let resp = with_oracle(|o| o.query(root, &manifest));
        assert!(actual.contains(&"package.json5".to_string()));
        assert!(!resp.npm.contains(&"package.json5".to_string()));
        observed.insert(
            classify_divergence(root, &manifest, "package.json5", true)
                .expect("AlternateRootManifest witness"),
        );
    }

    // 3. PnpmConfigPath
    {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let manifest = json!({
            "name": "pkg",
            "version": "1.0.0",
            "files": ["index.js"],
            "pnpm": { "config": "config/settings.json" }
        });
        write(root, "package.json", &manifest.to_string());
        touch(root, "index.js");
        touch(root, "config/settings.json");
        let actual = packlist(root, &manifest).unwrap();
        let resp = with_oracle(|o| o.query(root, &manifest));
        assert!(actual.contains(&"config/settings.json".to_string()));
        assert!(!resp.npm.contains(&"config/settings.json".to_string()));
        observed.insert(
            classify_divergence(root, &manifest, "config/settings.json", true)
                .expect("PnpmConfigPath witness"),
        );
    }

    // 4. AlwaysExcludedVcsDirectories
    {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let manifest = json!({
            "name": "pkg",
            "version": "1.0.0",
            "files": ["**"]
        });
        write(root, "package.json", &manifest.to_string());
        touch(root, "CVS/Root");
        let actual = packlist(root, &manifest).unwrap();
        let resp = with_oracle(|o| o.query(root, &manifest));
        assert!(!actual.contains(&"CVS/Root".to_string()));
        assert!(resp.npm.contains(&"CVS/Root".to_string()));
        observed.insert(
            classify_divergence(root, &manifest, "CVS/Root", false)
                .expect("AlwaysExcludedVcsDirectories witness"),
        );
    }

    // 5. NamedFileDepthRule (pnpm/pnpm#16920)
    {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let manifest = json!({
            "name": "pkg",
            "version": "1.0.0",
            "files": ["src", "src/util/format.test.js"]
        });
        write(root, "package.json", &manifest.to_string());
        write(root, "src/.npmignore", "*.test.js\nutil/\n");
        touch(root, "src/index.js");
        touch(root, "src/util/format.test.js");
        let actual = packlist(root, &manifest).unwrap();
        let resp = with_oracle(|o| o.query(root, &manifest));
        assert!(actual.contains(&"src/util/format.test.js".to_string()));
        assert!(!resp.npm.contains(&"src/util/format.test.js".to_string()));
        observed.insert(
            classify_divergence(root, &manifest, "src/util/format.test.js", true)
                .expect("NamedFileDepthRule witness"),
        );
    }

    // 6. ExactCaseEntryPoint (pnpm/pnpm#16918)
    {
        let dir = tempdir().unwrap();
        let root = dir.path();
        touch(root, "dist/UPPER.js");
        if root.join("dist/upper.js").exists() {
            observed.insert(DeliberateDivergence::ExactCaseEntryPoint);
        } else {
            let manifest = json!({
                "name": "pkg",
                "version": "1.0.0",
                "files": [],
                "main": "dist/upper.js"
            });
            write(root, "package.json", &manifest.to_string());
            let actual = packlist(root, &manifest).unwrap();
            let resp = with_oracle(|o| o.query(root, &manifest));
            assert!(!actual.contains(&"dist/UPPER.js".to_string()));
            assert!(resp.npm.contains(&"dist/UPPER.js".to_string()));
            observed.insert(
                classify_divergence(root, &manifest, "dist/UPPER.js", false)
                    .expect("ExactCaseEntryPoint witness"),
            );
        }
    }

    // 7. UnanchoredFilesFieldNegation
    {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let manifest = json!({
            "name": "pkg",
            "version": "1.0.0",
            "files": ["lib", "!*.map"]
        });
        observed.insert(
            classify_divergence(root, &manifest, "lib/index.js.map", false)
                .expect("UnanchoredFilesFieldNegation witness"),
        );
    }

    let expected: BTreeSet<DeliberateDivergence> = DELIBERATE_DIVERGENCES
        .iter()
        .copied()
        .collect();
    assert_eq!(observed, expected);
}

#[derive(Debug, Clone, Serialize)]
struct GeneratedFixture {
    files: Vec<String>,
    ignore_files: Vec<(String, String)>,
    internal_file_link: Option<(String, String)>,
    escaping_link: bool,
    manifest: Value,
}

fn dir_segment_strategy() -> impl Strategy<Value = &'static str> {
    select(
        &[
            "src",
            "lib",
            "dist",
            "sub",
            "nested",
            "deep",
            ".hidden",
            "UPPER",
            "MixedCase",
            ".git",
            "CVS",
            "node_modules",
        ][..],
    )
}

fn file_basename_strategy() -> impl Strategy<Value = &'static str> {
    select(
        &[
            "index.js",
            "INDEX.JS",
            "Index.Ts",
            "index.test.js",
            "app.mjs",
            "a.js",
            "[a].js",
            "a",
            "b",
            "foo",
            "foofoo",
            "part1.js",
            "part2.js",
            "part3.js",
            "part01.js",
            "part02.js",
            "parta.js",
            "partb.js",
            ".env",
            ".foo",
            ".npmrc",
            ".DS_Store",
            "._index.js",
            "._foo",
            "foo.swp",
            ".foo.swp",
            "foo.swo",
            "foo.orig",
            "foo~",
            "#foo#",
            "core",
            "npm-debug.log",
            "Thumbs.db",
            "package-lock.json",
            "yarn.lock",
            "pnpm-lock.yaml",
            "bun.lockb",
            "README.md",
            "readme.txt",
            "Readme~",
            "LICENSE",
            "licence.txt",
            "COPYING",
        ][..],
    )
}

fn rel_file_path_strategy() -> impl Strategy<Value = String> {
    (collection::vec(dir_segment_strategy(), 0..=4), file_basename_strategy()).prop_map(
        |(dirs, base)| {
            if dirs.is_empty() { base.to_string() } else { format!("{}/{base}", dirs.join("/")) }
        },
    )
}

fn ignore_rule_strategy() -> impl Strategy<Value = &'static str> {
    select(
        &[
            "*.test.js",
            "!*.test.js",
            "*.js",
            "!index.js",
            "!/index.js",
            "dist/",
            "lib/",
            "src/**",
            "!lib/index.js",
            "!dist/**",
            "*.orig",
            "!*.orig",
            "!.DS_Store",
            "!npm-debug.log",
            "!yarn.lock",
            "part{1..3}.js",
            "part{01..02}.js",
            "part{a..b}.js",
            "sub/",
            "!sub/index.js",
            "# comment",
        ][..],
    )
}

fn ignore_file_strategy() -> impl Strategy<Value = (String, String)> {
    (
        collection::vec(select(&["src", "lib", "dist", "sub", "nested"][..]), 0..=3),
        select(&[".npmignore", ".gitignore"][..]),
        collection::vec(ignore_rule_strategy(), 1..=5),
    )
        .prop_map(|(dirs, filename, rules)| {
            let path = if dirs.is_empty() {
                filename.to_string()
            } else {
                format!("{}/{filename}", dirs.join("/"))
            };
            (path, format!("{}\n", rules.join("\n")))
        })
}

fn files_entry_strategy() -> impl Strategy<Value = &'static str> {
    select(
        &[
            "**",
            "lib",
            "lib/*",
            "lib/**",
            "dist/**",
            "src/**",
            "!dist/index.js",
            "!lib/*.test.js",
            "*.js",
            "!*.test.js",
            "index.js",
            "./index.js",
            "./!(*.test.js)",
            "@(a|b)",
            "+(foo)",
            "[a].js",
            "part{1..3}.js",
            "part{01..02}.js",
            "part{a..b}.js",
            "file.orig",
            ".DS_Store",
        ][..],
    )
}

fn bin_value_for_kind(kind: u8) -> Value {
    if kind == 0 {
        json!("lib/index.js")
    } else {
        json!({ "cli": "lib/index.js", " helper": "./dist/index.js" })
    }
}

fn build_manifest(
    files: Option<Vec<&'static str>>,
    main: Option<&'static str>,
    browser: Option<&'static str>,
    bin_kind: Option<u8>,
) -> Value {
    let mut map = serde_json::Map::from_iter([
        ("name".to_string(), json!("generated-pkg")),
        ("version".to_string(), json!("1.0.0")),
    ]);
    for (key, val) in [
        ("files", files.map(|v| json!(v))),
        ("main", main.map(|v| json!(v))),
        ("browser", browser.map(|v| json!(v))),
        ("bin", bin_kind.map(bin_value_for_kind)),
    ] {
        if let Some(val) = val {
            map.insert(key.to_string(), val);
        }
    }
    Value::Object(map)
}

fn manifest_strategy() -> impl Strategy<Value = Value> {
    (
        option::of(collection::vec(files_entry_strategy(), 0..=5)),
        option::of(select(&["index.js", "./dist/index.js", "lib/index.js", "../escape.js"][..])),
        option::of(select(&["dist/index.js", "./lib/index.js"][..])),
        option::of(select(&[0u8, 1u8][..])),
    )
        .prop_map(|(files, main, browser, bin_kind)| build_manifest(files, main, browser, bin_kind))
}

fn fixture_strategy() -> impl Strategy<Value = GeneratedFixture> {
    (
        collection::vec(rel_file_path_strategy(), 1..=25),
        collection::vec(ignore_file_strategy(), 0..=4),
        option::of(Just(("sub/internal-link.js".to_string(), "../index.js".to_string()))),
        any::<bool>(),
        manifest_strategy(),
    )
        .prop_map(|(files, ignore_files, internal_file_link, escaping_link, manifest)| {
            GeneratedFixture { files, ignore_files, internal_file_link, escaping_link, manifest }
        })
}

struct OccupiedTree {
    files: BTreeSet<PathBuf>,
    dirs: BTreeSet<PathBuf>,
}

impl OccupiedTree {
    fn new() -> Self {
        Self { files: BTreeSet::from([PathBuf::from("package.json")]), dirs: BTreeSet::new() }
    }

    fn can_place(&self, path: &Path) -> bool {
        !self.files.contains(path)
            && !self.dirs.contains(path)
            && !path
                .ancestors()
                .skip(1)
                .any(|ancestor| self.files.contains(ancestor))
    }

    fn claim_file(&mut self, rel: &str) -> bool {
        let path = Path::new(rel);
        if !self.can_place(path) {
            return false;
        }
        for ancestor in path.ancestors().skip(1) {
            if !ancestor.as_os_str().is_empty() {
                self.dirs.insert(ancestor.to_path_buf());
            }
        }
        self.files.insert(path.to_path_buf());
        true
    }

    fn write_claimed(&mut self, root: &Path, rel: &str, content: &str) {
        if self.claim_file(rel) {
            write(root, rel, content);
        }
    }

    fn place_links(&mut self, root: &Path, fixture: &GeneratedFixture) {
        if let Some((link_rel, target_rel)) = &fixture.internal_file_link
            && self.claim_file(link_rel)
        {
            self.write_claimed(root, "index.js", "");
            place_fixture_symlink(&root.join(link_rel), Path::new(target_rel));
        }
        if fixture.escaping_link && self.claim_file("escape-link.js") {
            place_fixture_symlink(
                &root.join("escape-link.js"),
                Path::new("../../outside-secret.js"),
            );
        }
    }
}

fn materialize_fixture(root: &Path, fixture: &GeneratedFixture) {
    write(root, "package.json", &fixture.manifest.to_string());
    let mut occupied = OccupiedTree::new();
    for rel in &fixture.files {
        occupied.write_claimed(root, rel, "");
    }
    for (rel, content) in &fixture.ignore_files {
        occupied.write_claimed(root, rel, content);
    }
    occupied.place_links(root, fixture);
}

fn place_fixture_symlink(full_link: &Path, target_rel: &Path) {
    if let Some(parent) = full_link.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = pnpm_fs::create_symlink(target_rel, full_link, false);
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(64),
        ..ProptestConfig::default()
    })]

    #[test]
    fn generated_trees_match_npm_packlist(fixture in fixture_strategy()) {
        let dir = tempdir().unwrap();
        materialize_fixture(dir.path(), &fixture);
        assert_matches_oracles(
            dir.path(),
            &fixture.manifest,
            &serde_json::to_string(&fixture).unwrap(),
        );
    }
}

#[test]
fn pack_matches_npm_pack_dry_run() {
    let batch_tmp = tempdir().unwrap();
    let mut dirs_to_pack: Vec<(String, PathBuf, Value)> = Vec::new();

    // 1. Generated fixtures (without internal symlinks since `npm pack` tars files)
    let sample_fixtures = [
        GeneratedFixture {
            files: vec![
                "index.js".into(),
                "index.test.js".into(),
                "dist/app.js".into(),
                "dist/app.js.map".into(),
                "src/main.ts".into(),
                ".DS_Store".into(),
                "._index.js".into(),
                "foo.swp".into(),
                "README.md".into(),
                "LICENSE".into(),
            ],
            ignore_files: vec![
                (".gitignore".into(), "*.test.js\n*.map\n".into()),
                ("dist/.npmignore".into(), "!*.map\n".into()),
            ],
            internal_file_link: None,
            escaping_link: true,
            manifest: json!({
                "name": "gen-sample-1",
                "version": "1.0.0",
                "files": ["dist/**", "index.js"],
                "main": "dist/app.js"
            }),
        },
        GeneratedFixture {
            files: vec![
                "lib/a.js".into(),
                "lib/b.test.js".into(),
                "lib/sub/deep.js".into(),
                "part1.js".into(),
                "part2.js".into(),
                "part3.js".into(),
                "yarn.lock".into(),
                "lib/yarn.lock".into(),
            ],
            ignore_files: vec![
                (".npmignore".into(), "part{1..2}.js\n".into()),
                ("lib/.gitignore".into(), "*.test.js\n".into()),
            ],
            internal_file_link: None,
            escaping_link: false,
            manifest: json!({
                "name": "gen-sample-2",
                "version": "1.0.0",
                "bin": { "gen-sample-2": "lib/a.js" }
            }),
        },
    ];

    for (idx, fixture) in sample_fixtures.iter().enumerate() {
        let pkg_name = format!("gen-fixture-{idx}");
        let pkg_dir = batch_tmp.path().join(&pkg_name);
        fs::create_dir_all(&pkg_dir).unwrap();
        let mut manifest = fixture.manifest.clone();
        manifest["name"] = json!(pkg_name);
        let mut adjusted = fixture.clone();
        adjusted.manifest = manifest.clone();
        materialize_fixture(&pkg_dir, &adjusted);
        dirs_to_pack.push((pkg_name, pkg_dir, manifest));
    }

    // 2. A slice of real packages (from node_modules and workspace packages)
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let real_candidates = [
        repo_root.join("pnpm11/fs/packlist/node_modules/npm-packlist"),
        repo_root.join("pnpm11/fs/packlist/node_modules/ignore-walk"),
        repo_root.join("pnpm11/fs/packlist/node_modules/npm-bundled"),
        repo_root.join("pnpm11/fs/packlist/node_modules/minimatch"),
        repo_root.join("pnpm11/fs/packlist/node_modules/glob"),
        repo_root.join("pnpm11/fs/packlist"),
        repo_root.join("pnpm11/crypto/hash"),
        repo_root.join("pnpm11/text/comments-parser"),
    ];

    for (idx, candidate) in real_candidates.iter().enumerate() {
        let Ok(real_dir) = fs::canonicalize(candidate) else { continue };
        let Ok(raw_manifest) = fs::read_to_string(real_dir.join("package.json")) else { continue };
        let Ok(mut manifest) = serde_json::from_str::<Value>(&raw_manifest) else { continue };
        let pkg_name = format!("real-pkg-{idx}");
        let pkg_dir = batch_tmp.path().join(&pkg_name);
        copy_tree_without_node_modules(&real_dir, &pkg_dir);
        manifest["name"] = json!(pkg_name);
        manifest
            .as_object_mut()
            .unwrap()
            .remove("scripts");
        manifest
            .as_object_mut()
            .unwrap()
            .remove("bundleDependencies");
        manifest
            .as_object_mut()
            .unwrap()
            .remove("bundledDependencies");
        write(&pkg_dir, "package.json", &manifest.to_string());
        dirs_to_pack.push((pkg_name, pkg_dir, manifest));
    }

    let npm_pack_files = run_npm_pack_dry_run(
        batch_tmp.path(),
        &dirs_to_pack
            .iter()
            .map(|(_, dir, _)| dir.as_path())
            .collect::<Vec<_>>(),
    );

    for (pkg_name, pkg_dir, manifest) in &dirs_to_pack {
        let mut actual = packlist(pkg_dir, manifest).unwrap();
        actual.sort();
        let expected = npm_pack_files
            .get(pkg_name)
            .unwrap_or_else(|| panic!("missing npm pack output for {pkg_name}"));
        let actual_set: BTreeSet<&str> = actual
            .iter()
            .map(String::as_str)
            .collect();
        let expected_set: BTreeSet<&str> = expected
            .iter()
            .map(String::as_str)
            .collect();
        for path in actual_set.difference(&expected_set) {
            assert!(
                classify_divergence(pkg_dir, manifest, path, true).is_some(),
                "npm pack --dry-run missing path present in pnpm-fs-packlist for {pkg_name}: {path:?}\nactual: {actual:?}\nnpm_pack: {expected:?}",
            );
        }
        for path in expected_set.difference(&actual_set) {
            assert!(
                classify_divergence(pkg_dir, manifest, path, false).is_some(),
                "npm pack --dry-run included path excluded by pnpm-fs-packlist for {pkg_name}: {path:?}\nactual: {actual:?}\nnpm_pack: {expected:?}",
            );
        }
    }
}

fn copy_tree_without_node_modules(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    let Ok(entries) = fs::read_dir(src) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name == "node_modules" || name == ".git" {
            continue;
        }
        let Ok(ft) = entry.file_type() else { continue };
        let dst_path = dst.join(&name);
        if ft.is_dir() {
            copy_tree_without_node_modules(&entry.path(), &dst_path);
        } else if ft.is_file() {
            let _ = fs::copy(entry.path(), dst_path);
        }
    }
}

fn run_npm_pack_dry_run(cwd: &Path, pkg_dirs: &[&Path]) -> BTreeMap<String, Vec<String>> {
    let npm_bin = if cfg!(windows) { "npm.cmd" } else { "npm" };
    let output = Command::new(npm_bin)
        .arg("pack")
        .arg("--dry-run")
        .arg("--json")
        .arg("--ignore-scripts")
        .args(pkg_dirs)
        .current_dir(cwd)
        .output()
        .expect("spawn npm pack --dry-run --json");
    assert!(
        output.status.success(),
        "npm pack --dry-run failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let parsed: Value = serde_json::from_slice(&output.stdout).expect("parse npm pack json");
    let mut out = BTreeMap::new();
    let entries: Vec<&Value> = match &parsed {
        Value::Array(arr) => arr.iter().collect(),
        Value::Object(map) => map.values().collect(),
        other => panic!("unexpected npm pack --json shape: {other}"),
    };
    for entry in entries {
        let name = entry["name"]
            .as_str()
            .expect("package name in npm pack output")
            .to_string();
        let mut files: Vec<String> = entry["files"]
            .as_array()
            .expect("files array in npm pack output")
            .iter()
            .filter_map(|f| f["path"].as_str().map(String::from))
            .collect();
        files.sort();
        out.insert(name, files);
    }
    out
}

#[test]
fn file_selection_matches_typescript_packlist() {
    let mut fixtures = Vec::new();
    let mut inputs = Vec::new();
    let mut actual = Vec::new();
    for (manifest, ignore_kind) in manifests()
        .into_iter()
        .flat_map(|manifest| (0..5).map(move |kind| (manifest.clone(), kind)))
    {
        let dir = tempdir().unwrap();
        let root = dir.path();
        for file in [
            "index.js",
            "index.test.js",
            "a.js",
            "[a].js",
            "lib/foo",
            "!foo",
            "a",
            "b",
            "foo",
            "foofoo",
            "part-02.js",
            "part-01.js",
            "part000.js",
            "part001.js",
            "part01.js",
            "part02.js",
            "part03.js",
            "part04.js",
            "part05.js",
            "part06.js",
            "part07.js",
            "part08.js",
            "part09.js",
            "part10.js",
            "part1.js",
            "part2.js",
            "part3.js",
            "part4.js",
            "parta.js",
            "partb.js",
            "partc.js",
            "partd.js",
            "part1a.js",
            "part2b.js",
            "part3a.js",
            "nested/part2.js",
            "README.md",
            "LICENSE",
            "dist/index.js",
            "dist/other.js",
            "dist/lib/app.js",
            "dist/lib/chunk.js",
            "lib/index.js",
            "lib/index.test.js",
            "lib/a",
            "lib/b",
            "lib/.env",
            "npm-debug.log",
            ".DS_Store",
            "file.orig",
            "yarn.lock",
            "pnpm-lock.yaml",
            "package-lock.json",
            "lib/yarn.lock",
            "lib/file.orig",
            "lib/npm-debug.log",
        ] {
            touch(root, file);
        }
        write(root, "package.json", &manifest.to_string());
        fs::create_dir(root.join("sub")).unwrap();
        pnpm_fs::symlink_dir(&root.join("lib"), &root.join("sub/link")).unwrap();
        write_root_ignore_files(root, ignore_kind);
        write(root, "dist/.npmignore", "index.js\n");
        write(root, "lib/.gitignore", "*.test.js\n");
        write(
            root,
            "node_modules/bundled/package.json",
            r#"{"name":"bundled","files":["lib/**"]}"#,
        );
        touch(root, "node_modules/bundled/lib/index.js");
        touch(root, "node_modules/bundled/lib/file.orig");
        inputs.push(json!({ "root": root, "manifest": manifest }));
        actual.push(packlist(root, &manifest).unwrap());
        fixtures.push(dir);
    }
    let mut oracle = Command::new("node")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tests/parity.mjs"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    oracle.stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&inputs).unwrap())
        .unwrap();
    let output = oracle.wait_with_output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let expected: Vec<Vec<String>> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(expected.len(), inputs.len());
    for ((input, actual), expected) in inputs.iter().zip(actual).zip(expected) {
        assert_eq!(actual, expected, "input: {input}");
    }
}

fn write_root_ignore_files(root: &Path, kind: u8) {
    let range_ignore = "part{03..01..2}.js\n";
    write(root, ".gitignore", if kind == 4 { range_ignore } else { "dist/\n" });
    write(
        root,
        ".npmignore",
        if kind == 3 {
            range_ignore
        } else {
            "dist/\n!npm-debug.log\n!file.orig\n!.DS_Store\n!yarn.lock\n"
        },
    );
    if !matches!(kind, 0 | 3) {
        fs::remove_file(root.join(".npmignore")).unwrap();
    }
    if kind == 2 {
        fs::remove_file(root.join(".gitignore")).unwrap();
    }
}

fn manifests() -> Vec<Value> {
    let mut manifests = vec![json!({}), json!({ "bundleDependencies": ["bundled"] })];
    for files in [
        json!([]),
        json!([""]),
        json!(["./"]),
        json!(["**"]),
        json!(["lib"]),
        json!(["lib/*"]),
        json!(["foo", "lib/*.js"]),
        json!(["./foo", "lib/*.js"]),
        json!(["./!(*.test.js)"]),
        json!(["./@(a|b)", "lib/*.js"]),
        json!(["./!foo"]),
        json!(["[a].js"]),
        json!(["@(a|b)"]),
        json!(["+(foo)"]),
        json!(["lib/!(*.test.js)"]),
        json!(["!(*.test.js)"]),
        json!(["**", "!dist", "dist/index.js"]),
        json!(["**", "!sub", "sub/link"]),
        json!(["file.orig", "npm-debug.log", ".DS_Store", "yarn.lock", "lib/yarn.lock"]),
        json!(["part{1..3}.js"]),
        json!(["part{3..1}.js"]),
        json!(["part{1..5..2}.js"]),
        json!(["part{1..5..-2}.js"]),
        json!(["part{1..3..0}.js"]),
        json!(["part{1..10..02}.js"]),
        json!(["part{10..1..-02}.js"]),
        json!(["part{01..03}.js"]),
        json!(["part{-03..01}.js"]),
        json!(["part{1..3}{a,b}.js"]),
        json!(["part{a..c}.js"]),
        json!(["part{d..a..2}.js"]),
        json!(["*.js", "!part{1..3}.js"]),
    ] {
        manifests.push(json!({ "files": files }));
        manifests.push(json!({ "files": files, "bundleDependencies": ["bundled"] }));
        manifests.push(json!({ "files": files, "main": "dist/index.js", "browser": "dist/lib/app.js", "bin": { "x": "lib/index.js" } }));
    }
    manifests
}
