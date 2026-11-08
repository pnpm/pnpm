use crate::Lockfile;
use pretty_assertions::assert_eq;
use std::path::Path;

fn parse(yaml: &str) -> Lockfile {
    Lockfile::parse(yaml, Path::new("pnpm-lock.yaml"))
        .expect("parse the lockfile")
        .expect("the lockfile has content")
}

const OPTIONAL_PEER_LOCKFILE: &str = text_block_macros::text_block! {
    "lockfileVersion: '9.0'"
    ""
    "importers:"
    ""
    "  .:"
    "    dependencies:"
    "      buffer-impl:"
    "        specifier: ^3.7.0"
    "        version: 3.7.1"
    "      container:"
    "        specifier: ^3.2.0"
    "        version: 3.2.2(buffer-impl@3.7.1)"
    ""
    "packages:"
    ""
    "  buffer-impl@3.7.1:"
    "    resolution: {integrity: sha512-AAAA}"
    ""
    "  container@3.2.2:"
    "    resolution: {integrity: sha512-BBBB}"
    ""
    "  fs-impl@4.8.4:"
    "    resolution: {integrity: sha512-CCCC}"
    "    peerDependencies:"
    "      buffer-impl: '*'"
    "    peerDependenciesMeta:"
    "      buffer-impl:"
    "        optional: true"
    ""
    "  path-impl@3.1.2:"
    "    resolution: {integrity: sha512-DDDD}"
    ""
    "snapshots:"
    ""
    "  buffer-impl@3.7.1: {}"
    ""
    "  container@3.2.2(buffer-impl@3.7.1):"
    "    dependencies:"
    "      fs-impl: 4.8.4(buffer-impl@3.7.1)"
    "    transitivePeerDependencies:"
    "      - abort-impl"
    "      - buffer-impl"
    ""
    "  fs-impl@4.8.4(buffer-impl@3.7.1):"
    "    dependencies:"
    "      path-impl: 3.1.2"
    "    optionalDependencies:"
    "      buffer-impl: 3.7.1"
    ""
    "  path-impl@3.1.2: {}"
};

#[test]
fn optional_peer_bound_from_the_root_round_trips() {
    let (yaml, _) = written_and_read(&parse(OPTIONAL_PEER_LOCKFILE));
    let snapshots = &yaml[yaml.find("\nsnapshots:").expect("a snapshots section")..];
    assert_eq!(
        snapshots,
        text_block_macros::text_block_fnl! {
            ""
            "snapshots:"
            ""
            "  buffer-impl@3.7.1: {}"
            ""
            "  container@3.2.2:"
            "    dependencies:"
            "      fs-impl: 4.8.4"
            "    transitivePeerDependencies:"
            "      buffer-impl: 3.7.1"
            "    missingPeerDependencies:"
            "      - abort-impl"
            ""
            "  fs-impl@4.8.4:"
            "    peerDependencies:"
            "      buffer-impl: 3.7.1"
            "    dependencies:"
            "      path-impl: 3.1.2"
            ""
            "  path-impl@3.1.2: {}"
        },
    );
}

/// Writes `lockfile` in the next format and reads it back.
fn written_and_read(lockfile: &Lockfile) -> (String, Lockfile) {
    let lockfile = as_next(lockfile.clone());
    let yaml = lockfile.to_yaml_string().expect("render the next format");
    let read = Lockfile::parse(&yaml, Path::new("pnpm-lock.yaml"))
        .expect("parse the next format")
        .expect("the lockfile has content");
    assert_eq!(read, lockfile);
    (yaml, read)
}

#[test]
fn package_that_is_the_peer_of_its_own_dependency_round_trips() {
    let lockfile = parse(text_block_macros::text_block! {
        "lockfileVersion: '9.0'"
        ""
        "importers:"
        ""
        "  .:"
        "    dependencies:"
        "      core:"
        "        specifier: 1.0.0"
        "        version: 1.0.0"
        "      core-next:"
        "        specifier: npm:core@2.0.0"
        "        version: core@2.0.0"
        ""
        "packages:"
        ""
        "  core@1.0.0:"
        "    resolution: {integrity: sha512-AAAA}"
        ""
        "  core@2.0.0:"
        "    resolution: {integrity: sha512-BBBB}"
        ""
        "  helper@1.0.0:"
        "    resolution: {integrity: sha512-CCCC}"
        "    peerDependencies:"
        "      core: '*'"
        ""
        "snapshots:"
        ""
        "  core@1.0.0:"
        "    dependencies:"
        "      helper: 1.0.0(core@1.0.0)"
        ""
        "  core@2.0.0:"
        "    dependencies:"
        "      helper: 1.0.0(core@2.0.0)"
        ""
        "  helper@1.0.0(core@1.0.0):"
        "    dependencies:"
        "      core: 1.0.0"
        ""
        "  helper@1.0.0(core@2.0.0):"
        "    dependencies:"
        "      core: 2.0.0"
    });
    let (yaml, _) = written_and_read(&lockfile);
    assert!(yaml.contains("      helper: 1.0.0\n"), "{yaml}");
}

/// `parent` binds no peers, so its scope does not say which variant of
/// `child` it links. The writer keeps that reference's peer groups.
#[test]
fn reference_the_scope_does_not_select_keeps_its_peer_groups() {
    let lockfile = parse(text_block_macros::text_block! {
        "lockfileVersion: '9.0'"
        ""
        "importers:"
        ""
        "  .:"
        "    dependencies:"
        "      child:"
        "        specifier: 1.0.0"
        "        version: 1.0.0(react@17.0.0)"
        "      parent:"
        "        specifier: 1.0.0"
        "        version: 1.0.0"
        "      react:"
        "        specifier: 17.0.0"
        "        version: 17.0.0"
        ""
        "packages:"
        ""
        "  child@1.0.0:"
        "    resolution: {integrity: sha512-AAAA}"
        "    peerDependencies:"
        "      react: '*'"
        ""
        "  parent@1.0.0:"
        "    resolution: {integrity: sha512-BBBB}"
        ""
        "  react@17.0.0:"
        "    resolution: {integrity: sha512-CCCC}"
        ""
        "  react@18.0.0:"
        "    resolution: {integrity: sha512-DDDD}"
        ""
        "snapshots:"
        ""
        "  child@1.0.0(react@17.0.0):"
        "    dependencies:"
        "      react: 17.0.0"
        ""
        "  child@1.0.0(react@18.0.0):"
        "    dependencies:"
        "      react: 18.0.0"
        ""
        "  parent@1.0.0:"
        "    dependencies:"
        "      child: 1.0.0(react@18.0.0)"
        ""
        "  react@17.0.0: {}"
        ""
        "  react@18.0.0: {}"
    });
    let (yaml, _) = written_and_read(&lockfile);
    assert!(yaml.contains("      child: 1.0.0(react@18.0.0)\n"), "{yaml}");
    assert!(yaml.contains("        version: 1.0.0\n"), "{yaml}");
}

#[test]
fn current_lockfile_stays_suffixed_under_the_next_version() {
    let lockfile = as_next(parse(OPTIONAL_PEER_LOCKFILE));
    let dir = tempfile::tempdir().expect("create a temporary directory");
    lockfile.save_current_to_install_state_dir(dir.path()).expect("save the current lockfile");
    let text = std::fs::read_to_string(dir.path().join(Lockfile::CURRENT_FILE_NAME))
        .expect("read the current lockfile");
    assert!(text.starts_with("lockfileVersion: '10.0'"), "{text}");
    assert!(text.contains("  fs-impl@4.8.4(buffer-impl@3.7.1):"), "{text}");
    let read = Lockfile::load_current_from_install_state_dir(dir.path())
        .expect("load the current lockfile")
        .expect("the current lockfile has content");
    assert_eq!(read, lockfile);
}

/// Round-trips every lockfile listed in `PNPM_NEXT_LOCKFILE_CORPUS`,
/// separated by the platform's path separator.
#[test]
#[ignore = "reads lockfiles from outside the repository"]
fn corpus_round_trips() {
    let corpus = std::env::var_os("PNPM_NEXT_LOCKFILE_CORPUS").expect("PNPM_NEXT_LOCKFILE_CORPUS");
    let corpus = std::fs::read_to_string(corpus).expect("read the corpus list");
    let mut failures = Vec::new();
    let mut converted = 0;
    for path in corpus.lines().map(Path::new) {
        let Ok(text) = std::fs::read_to_string(path) else { continue };
        let Ok(Some(lockfile)) = Lockfile::parse(&text, path) else { continue };
        if !parallel_parse_matches_whole(&text, &lockfile) {
            failures.push(format!("{}: the parallel parse differs", path.display()));
        }
        let lockfile = as_next(lockfile);
        let yaml = match lockfile.to_yaml_string() {
            Ok(yaml) => yaml,
            Err(error) => {
                failures.push(format!("{}: {error}", path.display()));
                continue;
            }
        };
        let parsed = Lockfile::parse(&yaml, path).expect("parse the next format").expect("content");
        if parsed == lockfile {
            converted += 1;
        } else {
            failures.push(format!("{}: the next format reads back differently", path.display()));
        }
    }
    eprintln!("converted {converted}, failed {}", failures.len());
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn as_next(mut lockfile: Lockfile) -> Lockfile {
    lockfile.lockfile_version = crate::LockfileVersion::<9>::try_from(crate::ComVer::new(10, 0))
        .expect("the next major is supported");
    lockfile
}

/// Prints parse and render times of both formats for each lockfile in
/// `PNPM_NEXT_LOCKFILE_BENCH`, one path per line. The minimum of the runs.
#[test]
#[ignore = "timing aid"]
fn compare_timings() {
    let list = std::env::var("PNPM_NEXT_LOCKFILE_BENCH").expect("PNPM_NEXT_LOCKFILE_BENCH");
    let list = std::fs::read_to_string(list).expect("read the list");
    let runs = 15;
    let min_of = |work: &dyn Fn()| {
        (0..runs)
            .map(|_| {
                let started = std::time::Instant::now();
                work();
                started.elapsed()
            })
            .min()
            .expect("at least one run")
    };
    for path in list.lines().map(Path::new) {
        let v9_text = std::fs::read_to_string(path).expect("read");
        let v9 = Lockfile::parse(&v9_text, path).expect("parse").expect("content");
        let next = as_next(v9.clone());
        let next_text = next.to_yaml_string().expect("render the next format");
        let parse_v9 = min_of(&|| drop(Lockfile::parse(&v9_text, path)));
        let parse_next = min_of(&|| drop(Lockfile::parse(&next_text, path)));
        let render_v9 = min_of(&|| drop(v9.to_yaml_string()));
        let render_next = min_of(&|| drop(next.to_yaml_string()));
        let snapshots = v9.snapshots.as_ref().map_or(0, std::collections::HashMap::len);
        eprintln!(
            "{}\n  snapshots {snapshots}, bytes v9 {} next {}\n  parse  v9 {parse_v9:?} next {parse_next:?}\n  render v9 {render_v9:?} next {render_next:?}",
            path.display(),
            v9_text.len(),
            next_text.len(),
        );
    }
}

/// Whether `lockfile`, parsed section by section, equals `text` parsed whole.
fn parallel_parse_matches_whole(text: &str, lockfile: &Lockfile) -> bool {
    let whole: Lockfile = serde_saphyr::from_str(&crate::extract_main_document(text))
        .expect("parse the document whole");
    whole.snapshots == lockfile.snapshots
        && whole.importers == lockfile.importers
        && whole.packages == lockfile.packages
        && whole.extra == lockfile.extra
}
