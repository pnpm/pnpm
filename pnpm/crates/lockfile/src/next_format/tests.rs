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
    "      abort-impl: '*'"
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
    let packages = &yaml[yaml.find("\n  \"packages\"").expect("a packages section")..];
    assert_eq!(
        packages,
        text_block_macros::text_block_fnl! {
            ""
            r#"  "packages": {"#
            r#"    "buffer-impl@3.7.1": {"#
            r#"      "integrity": "sha512-AAAA","#
            "    },"
            ""
            r#"    "container@3.2.2": {"#
            r#"      "integrity": "sha512-BBBB","#
            r#"      "peerDependencies": {"#
            r#"        "buffer-impl": {"version": "3.7.1", "below": true},"#
            "      },"
            r#"      "dependencies": {"#
            r#"        "fs-impl": "4.8.4","#
            "      },"
            "    },"
            ""
            r#"    "fs-impl@4.8.4": {"#
            r#"      "integrity": "sha512-CCCC","#
            r#"      "peerDependencies": {"#
            r#"        "abort-impl": "*","#
            r#"        "buffer-impl": {"specifier": "*", "version": "3.7.1", "optional": true},"#
            "      },"
            r#"      "dependencies": {"#
            r#"        "path-impl": "3.1.2","#
            "      },"
            "    },"
            ""
            r#"    "path-impl@3.1.2": {"#
            r#"      "integrity": "sha512-DDDD","#
            "    },"
            "  },"
            "}"
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
    assert!(yaml.contains("        \"helper\": \"1.0.0\",\n"), "{yaml}");
}

/// `parent` binds no peers, so its scope does not say which snapshot of
/// `child` it links. The writer names that snapshot by the binding that
/// tells `child`'s snapshots apart.
#[test]
fn reference_the_scope_does_not_select_names_its_snapshot() {
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
    assert!(
        yaml.contains(concat!(
            "        \"child\": {\n",
            "          \"version\": \"1.0.0\",\n",
            "          \"with\": {\n",
            "            \"react\": \"18.0.0\",\n",
            "          },\n",
            "        },\n",
        )),
        "{yaml}",
    );
    assert!(yaml.contains("        \"child\": \"1.0.0\",\n"), "{yaml}");
    assert!(yaml.contains("        \"parent\": \"1.0.0\",\n"), "{yaml}");
    assert!(!yaml.contains('('), "{yaml}");
}

/// A project's dependency that names its snapshot leaves out a version
/// that follows from the specifier, as a dependency without `with` does.
#[test]
fn importer_dependency_that_names_its_snapshot_omits_an_implied_version() {
    let lockfile = parse(text_block_macros::text_block! {
        "lockfileVersion: '9.0'"
        ""
        "importers:"
        ""
        "  .:"
        "    dependencies:"
        "      child:"
        "        specifier: 1.0.0"
        "        version: 1.0.0(react@18.0.0)"
        ""
        "  packages/a:"
        "    dependencies:"
        "      child:"
        "        specifier: ^1.0.0"
        "        version: 1.0.0(react@18.0.0)"
        ""
        "  packages/b:"
        "    dependencies:"
        "      parent:"
        "        specifier: 1.0.0"
        "        version: 1.0.0"
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
        "      child: 1.0.0(react@17.0.0)"
        ""
        "  react@17.0.0: {}"
        ""
        "  react@18.0.0: {}"
    });
    let (yaml, _) = written_and_read(&lockfile);
    let start = yaml.find("\n  \"projects\"").expect("an importers section");
    let end = yaml.find("\n  \"packages\"").expect("a packages section");
    assert_eq!(
        &yaml[start..end],
        text_block_macros::text_block_fnl! {
            ""
            r#"  "projects": {"#
            r#"    ".": {"#
            r#"      "dependencies": {"#
            r#"        "child": {"#
            r#"          "specifier": "1.0.0","#
            r#"          "with": {"#
            r#"            "react": "18.0.0","#
            "          },"
            "        },"
            "      },"
            "    },"
            ""
            r#"    "packages/a": {"#
            r#"      "dependencies": {"#
            r#"        "child": {"#
            r#"          "specifier": "^1.0.0","#
            r#"          "version": "1.0.0","#
            r#"          "with": {"#
            r#"            "react": "18.0.0","#
            "          },"
            "        },"
            "      },"
            "    },"
            ""
            r#"    "packages/b": {"#
            r#"      "dependencies": {"#
            r#"        "parent": "1.0.0","#
            "      },"
            "    },"
            "  },"
        },
    );
}

/// A patched package's key and references carry no patch hash: the hash is
/// in `patchedDependencies`, which the reader applies to the package.
#[test]
fn patched_packages_carry_no_hash() {
    let lockfile = parse(text_block_macros::text_block! {
        "lockfileVersion: '9.0'"
        ""
        "patchedDependencies:"
        "  foo@2.1.0: abc123"
        ""
        "importers:"
        ""
        "  .:"
        "    dependencies:"
        "      foo:"
        "        specifier: ^2.0.0"
        "        version: 2.1.0(patch_hash=abc123)"
        ""
        "packages:"
        ""
        "  foo@2.1.0:"
        "    resolution: {integrity: sha512-AAAA}"
        ""
        "snapshots:"
        ""
        "  foo@2.1.0(patch_hash=abc123): {}"
    });
    let (yaml, _) = written_and_read(&lockfile);
    assert!(
        yaml.contains("        \"foo\": {\"specifier\": \"^2.0.0\", \"version\": \"2.1.0\"},\n"),
        "{yaml}",
    );
    assert!(
        yaml.contains("\n    \"foo@2.1.0\": {\n      \"integrity\": \"sha512-AAAA\",\n    },\n"),
        "{yaml}",
    );
    assert!(!yaml.contains("patch_hash"), "{yaml}");
}

/// An importer's dependency is its specifier alone when the version follows
/// from it: an exact version, a `catalog:` entry, with the alias the catalog
/// names, an injected workspace package, through its `packages` entry, under
/// an alias too. A range writes both, and so does a workspace link to a
/// project the lockfile does not list, an exact version that a workspace
/// link stands in for, and a `file:` path that reads differently from the
/// project and from the lockfile's directory. A catalog entry follows the
/// same rule, with the bare version of an `npm:` alias.
#[test]
fn importer_versions_that_follow_from_the_specifier_are_not_written() {
    let lockfile = parse(text_block_macros::text_block! {
        "lockfileVersion: '9.0'"
        ""
        "catalogs:"
        "  default:"
        "    '@scope/renamed':"
        "      specifier: npm:@scope/real@1.0.0"
        "      version: 1.0.0"
        "    pinned:"
        "      specifier: 3.0.0"
        "      version: 3.0.0"
        "    tool:"
        "      specifier: ^2.0.0"
        "      version: 2.1.0"
        ""
        "importers:"
        ""
        "  .:"
        "    dependencies:"
        "      '@scope/renamed':"
        "        specifier: 'catalog:'"
        "        version: '@scope/real@1.0.0'"
        "      exact:"
        "        specifier: 1.2.3"
        "        version: 1.2.3"
        "      injected:"
        "        specifier: workspace:*"
        "        version: file:packages/injected"
        "      injected-alias:"
        "        specifier: workspace:injected@*"
        "        version: injected@file:packages/injected"
        "      pinned:"
        "        specifier: 'catalog:'"
        "        version: 3.0.0"
        "      ranged:"
        "        specifier: ^1.0.0"
        "        version: 1.2.3"
        "      shared:"
        "        specifier: workspace:*"
        "        version: link:../shared"
        "      tool:"
        "        specifier: 'catalog:'"
        "        version: 2.1.0"
        ""
        "  packages/app:"
        "    dependencies:"
        "      lib:"
        "        specifier: file:../lib"
        "        version: file:packages/lib"
        "      pinned-link:"
        "        specifier: 1.0.0"
        "        version: link:../pinned-link"
        ""
        "packages:"
        ""
        "  '@scope/real@1.0.0':"
        "    resolution: {integrity: sha512-AAAA}"
        ""
        "  exact@1.2.3:"
        "    resolution: {integrity: sha512-BBBB}"
        ""
        "  injected@file:packages/injected:"
        "    resolution: {directory: packages/injected, type: directory}"
        ""
        "  lib@file:packages/lib:"
        "    resolution: {directory: packages/lib, type: directory}"
        ""
        "  pinned@3.0.0:"
        "    resolution: {integrity: sha512-EEEE}"
        ""
        "  ranged@1.2.3:"
        "    resolution: {integrity: sha512-CCCC}"
        ""
        "  tool@2.1.0:"
        "    resolution: {integrity: sha512-DDDD}"
        ""
        "snapshots:"
        ""
        "  '@scope/real@1.0.0': {}"
        ""
        "  exact@1.2.3: {}"
        ""
        "  injected@file:packages/injected: {}"
        ""
        "  lib@file:packages/lib: {}"
        ""
        "  pinned@3.0.0: {}"
        ""
        "  ranged@1.2.3: {}"
        ""
        "  tool@2.1.0: {}"
    });
    let (yaml, _) = written_and_read(&lockfile);
    assert!(yaml.contains("\n    \"injected@file:packages/injected\": {},\n"), "{yaml}");
    assert!(!yaml.contains("resolution"), "{yaml}");
    let start = yaml.find("\n  \"settings\"").expect("a settings section");
    let end = yaml.find("\n  \"packages\"").expect("a packages section");
    assert_eq!(
        &yaml[start..end],
        text_block_macros::text_block_fnl! {
            ""
            r#"  "settings": {"#
            r#"    "catalogs": {"#
            r#"      "default": {"#
            r#"        "@scope/renamed": "npm:@scope/real@1.0.0","#
            r#"        "pinned": "3.0.0","#
            r#"        "tool": {"specifier": "^2.0.0", "version": "2.1.0"},"#
            "      },"
            "    },"
            "  },"
            ""
            r#"  "projects": {"#
            r#"    ".": {"#
            r#"      "dependencies": {"#
            r#"        "@scope/renamed": "catalog:","#
            r#"        "exact": "1.2.3","#
            r#"        "injected": "workspace:*","#
            r#"        "injected-alias": "workspace:injected@*","#
            r#"        "pinned": "catalog:","#
            r#"        "ranged": {"specifier": "^1.0.0", "version": "1.2.3"},"#
            r#"        "shared": {"specifier": "workspace:*", "version": "link:../shared"},"#
            r#"        "tool": "catalog:","#
            "      },"
            "    },"
            ""
            r#"    "packages/app": {"#
            r#"      "dependencies": {"#
            r#"        "lib": {"specifier": "file:../lib", "version": "file:packages/lib"},"#
            r#"        "pinned-link": {"specifier": "1.0.0", "version": "link:../pinned-link"},"#
            "      },"
            "    },"
            "  },"
        },
    );
}

/// A lockfile with a boot document keeps it: a YAML document before the
/// main one, which is JSON, as a two-document stream.
#[test]
fn next_format_keeps_the_boot_document() {
    let (yaml, lockfile) = written_and_read(&parse(OPTIONAL_PEER_LOCKFILE));
    let boot = text_block_macros::text_block_fnl! {
        "---"
        "lockfileVersion: '9.0'"
        "importers:"
        "  .:"
        "    configDependencies: {}"
        "packages: {}"
        "snapshots: {}"
        "---"
    };
    let combined = format!("{boot}{yaml}");
    let read = Lockfile::parse(&combined, Path::new("pnpm-lock.yaml"))
        .expect("parse the two documents")
        .expect("the lockfile has content");
    assert_eq!(read, lockfile);
    let dir = tempfile::tempdir().expect("create a temporary directory");
    let path = dir.path().join(Lockfile::FILE_NAME);
    std::fs::write(&path, &combined).expect("write the lockfile");
    lockfile
        .save_to_path(&path, crate::SymlinkedLockfile::Refuse)
        .expect("save over the two documents");
    assert_eq!(std::fs::read_to_string(&path).expect("read the lockfile"), combined);
}

/// A `workspace:` dependency linked to another project is its specifier
/// alone, and the project it links to records its `name`, under which the
/// reader finds the link's target. A link to a path that is no importer
/// writes its version.
#[test]
fn linked_workspace_dependency_is_its_specifier_and_names_its_project() {
    let lockfile = parse(text_block_macros::text_block! {
        "lockfileVersion: '9.0'"
        ""
        "importers:"
        ""
        "  .:"
        "    devDependencies:"
        "      '@scope/a':"
        "        specifier: workspace:*"
        "        version: link:packages/a"
        ""
        "  packages/a: {}"
        ""
        "  packages/b:"
        "    dependencies:"
        "      '@scope/a':"
        "        specifier: workspace:^"
        "        version: link:../a"
        "      a-alias:"
        "        specifier: 'workspace:@scope/a@*'"
        "        version: link:../a"
        "      tool:"
        "        specifier: workspace:*"
        "        version: link:../tool"
    });
    let (yaml, _) = written_and_read(&lockfile);
    let start = yaml.find("\n  \"projects\"").expect("an importers section");
    assert_eq!(
        &yaml[start..],
        text_block_macros::text_block_fnl! {
            ""
            r#"  "projects": {"#
            r#"    ".": {"#
            r#"      "devDependencies": {"#
            r#"        "@scope/a": "workspace:*","#
            "      },"
            "    },"
            ""
            r#"    "packages/a": {"#
            r#"      "name": "@scope/a","#
            "    },"
            ""
            r#"    "packages/b": {"#
            r#"      "dependencies": {"#
            r#"        "@scope/a": "workspace:^","#
            r#"        "a-alias": "workspace:@scope/a@*","#
            r#"        "tool": {"specifier": "workspace:*", "version": "link:../tool"},"#
            "      },"
            "    },"
            "  },"
            "}"
        },
    );
}

#[test]
fn json_reader_allows_a_trailing_comma_and_keeps_one_in_a_string() {
    let value = crate::json_document::from_json_str::<serde_json::Value>(
        text_block_macros::text_block_fnl! {
            "{"
            r#"  "a": "x,]","#
            r#"  "b": [1, 2,],"#
            r#"  "c": {"d": "\",}",},"#
            "}"
        },
    )
    .expect("parse");
    assert_eq!(value, serde_json::json!({"a": "x,]", "b": [1, 2], "c": {"d": r#"",}"#}}));
}

/// The section splitter expects pnpm's layout; a document on one line
/// takes the whole-document path.
#[test]
fn json_document_in_another_layout_is_parsed_whole() {
    let text = r#"{"lockfileVersion":"13.0","projects":{".":{"dependencies":{"a":"1.0.0"}}},"packages":{"a@1.0.0":{"integrity":"sha512-AAAA"}}}"#;
    let lockfile = parse(text);
    assert_eq!(lockfile.importers.len(), 1);
    assert_eq!(lockfile.packages.as_ref().map(std::collections::HashMap::len), Some(1));
    let (yaml, _) = written_and_read(&lockfile);
    assert!(
        yaml.contains("\n    \"a@1.0.0\": {\n      \"integrity\": \"sha512-AAAA\",\n    },\n"),
        "{yaml}",
    );
}

/// A 9.0 lockfile written as JSON, as a test or a tool may write it, is
/// today's format: JSON is YAML, and the `lockfileVersion` says which
/// format the document is in.
#[test]
fn json_document_with_todays_version_is_todays_format() {
    let text = r#"{"lockfileVersion":"9.0","projects":{".":{"dependencies":{"a":{"specifier":"1.0.0","version":"1.0.0"}}}},"packages":{"a@1.0.0":{"resolution":{"integrity":"sha512-AAAA"}}},"snapshots":{"a@1.0.0":{}}}"#;
    let lockfile = parse(text);
    assert!(!lockfile.is_next_format());
    assert_eq!(lockfile.snapshots.as_ref().map(std::collections::HashMap::len), Some(1));
    let yaml = lockfile.to_yaml_string().expect("render today's format");
    assert!(yaml.starts_with("lockfileVersion: '9.0'\n"), "{yaml}");
}

/// A catalog specifier of `npm:` alone, or one whose target starts with a
/// multi-byte character, is rejected rather than sliced through.
#[test]
fn catalog_entry_with_an_odd_npm_alias_is_rejected() {
    for specifier in ["npm:", "npm:\u{e9}"] {
        let text = format!(
            r#"{{"lockfileVersion": "13.0", "settings": {{"catalogs": {{"default": {{"renamed": "{specifier}"}}}}}}, "projects": {{".": {{}}}}}}"#,
        );
        let error = Lockfile::parse(&text, Path::new("pnpm-lock.yaml"))
            .expect_err("a catalog entry without a version");
        assert!(error.to_string().contains("nothing follows from the specifier"), "{error}");
    }
}

/// A registry entry that records neither a resolution nor an integrity
/// would install without a check, so it is an error rather than a package
/// without metadata.
#[test]
fn registry_entry_without_a_resolution_is_rejected() {
    let text = r#"{"lockfileVersion": "13.0", "projects": {".": {"dependencies": {"foo": "1.0.0"}}}, "packages": {"foo@1.0.0": {}}}"#;
    let error = Lockfile::parse(text, Path::new("pnpm-lock.yaml"))
        .expect_err("an entry without a resolution");
    assert!(error.to_string().contains("records no resolution for foo@1.0.0"), "{error}");
}

/// A catalog entry written as `npm:name`, without a version, implies none.
#[test]
fn catalog_entry_whose_version_does_not_follow_is_rejected() {
    let text = text_block_macros::text_block! {
        "lockfileVersion: '13.0'"
        ""
        "settings:"
        "  catalogs:"
        "    default:"
        "      renamed: npm:real"
        ""
        "importers:"
        ""
        "  .: {}"
    };
    let error = Lockfile::parse(text, Path::new("pnpm-lock.yaml"))
        .expect_err("a catalog entry without a version");
    let message = error.to_string();
    assert!(message.contains("renamed resolves to in the catalog default"), "{message}");
    assert!(message.contains("npm:real"), "{message}");
}

#[test]
fn current_lockfile_stays_suffixed_under_the_next_version() {
    let lockfile = as_next(parse(OPTIONAL_PEER_LOCKFILE));
    let dir = tempfile::tempdir().expect("create a temporary directory");
    lockfile.save_current_to_install_state_dir(dir.path()).expect("save the current lockfile");
    let text = std::fs::read_to_string(dir.path().join(Lockfile::CURRENT_FILE_NAME))
        .expect("read the current lockfile");
    assert!(text.starts_with("lockfileVersion: '13.0'"), "{text}");
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
        if parsed == without_unused_packages(lockfile) {
            converted += 1;
        } else {
            failures.push(format!("{}: the next format reads back differently", path.display()));
        }
    }
    eprintln!("converted {converted}, failed {}", failures.len());
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn as_next(mut lockfile: Lockfile) -> Lockfile {
    lockfile.lockfile_version = crate::LockfileVersion::<9>::try_from(crate::ComVer::new(13, 0))
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

/// `lockfile` as the next format can write it back: without the `packages`
/// entries no snapshot uses, and without `deprecated`, which it does not
/// record.
fn without_unused_packages(mut lockfile: Lockfile) -> Lockfile {
    let used: std::collections::HashSet<crate::PackageKey> = lockfile.snapshots
        .iter()
        .flatten()
        .map(|(key, _)| key.without_peer())
        .collect();
    if let Some(packages) = lockfile.packages.as_mut() {
        packages.retain(|key, _| used.contains(key));
        for metadata in packages.values_mut() {
            metadata.deprecated = None;
        }
        // The next format cannot say that a directory package has no
        // metadata: its resolution follows from the key.
        for key in &used {
            let crate::VersionPart::File(directory) = key.suffix.version() else { continue };
            packages
                .entry(key.clone())
                .or_insert_with(|| crate::PackageMetadata {
                    resolution: crate::LockfileResolution::Directory(crate::DirectoryResolution {
                        directory: directory.clone(),
                    }),
                    version: None,
                    engines: None,
                    cpu: None,
                    os: None,
                    libc: None,
                    deprecated: None,
                    has_bin: None,
                    prepare: None,
                    bundled_dependencies: None,
                    peer_dependencies: None,
                    peer_dependencies_meta: None,
                });
        }
    }
    if lockfile.packages.as_ref().is_some_and(std::collections::HashMap::is_empty) {
        lockfile.packages = None;
    }
    lockfile
}
