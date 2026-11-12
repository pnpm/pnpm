use crate::Lockfile;
use pretty_assertions::assert_eq;
use std::{fmt::Write as _, path::Path};

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
    let packages = &yaml[yaml.find("\n\t\"packages\"").expect("a packages section")..];
    assert_eq!(
        packages,
        text_block_macros::text_block_fnl! {
            ""
            r#"	"packages": {"#
            r#"		"buffer-impl@3.7.1": {"#
            r#"			"integrity": "sha512-AAAA","#
            "		},"
            ""
            r#"		"container@3.2.2": {"#
            r#"			"integrity": "sha512-BBBB","#
            r#"			"peerDependencies": {"#
            r#"				"buffer-impl": {"#
            r#"					"version": "3.7.1","#
            r#"					"below": true,"#
            "				},"
            "			},"
            r#"			"dependencies": {"#
            r#"				"fs-impl": "4.8.4","#
            "			},"
            "		},"
            ""
            r#"		"fs-impl@4.8.4": {"#
            r#"			"integrity": "sha512-CCCC","#
            r#"			"peerDependencies": {"#
            r#"				"abort-impl": "*","#
            r#"				"buffer-impl": {"#
            r#"					"specifier": "*","#
            r#"					"version": "3.7.1","#
            r#"					"optional": true,"#
            "				},"
            "			},"
            r#"			"dependencies": {"#
            r#"				"path-impl": "3.1.2","#
            "			},"
            "		},"
            ""
            r#"		"path-impl@3.1.2": {"#
            r#"			"integrity": "sha512-DDDD","#
            "		},"
            "	},"
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
    assert!(yaml.contains("\t\t\t\t\"helper\": \"1.0.0\",\n"), "{yaml}");
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
            "\t\t\t\t\"child\": {\n",
            "\t\t\t\t\t\"version\": \"1.0.0\",\n",
            "\t\t\t\t\t\"with\": {\n",
            "\t\t\t\t\t\t\"react\": \"18.0.0\",\n",
            "					},\n",
            "				},\n",
        )),
        "{yaml}",
    );
    assert!(yaml.contains("\t\t\t\t\"child\": \"1.0.0\",\n"), "{yaml}");
    assert!(yaml.contains("\t\t\t\t\"parent\": \"1.0.0\",\n"), "{yaml}");
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
    let start = yaml.find("\n\t\"projects\"").expect("an importers section");
    let end = yaml.find("\n\t\"packages\"").expect("a packages section");
    assert_eq!(
        &yaml[start..end],
        text_block_macros::text_block_fnl! {
            ""
            r#"	"projects": {"#
            r#"		".": {"#
            r#"			"dependencies": {"#
            r#"				"child": {"#
            r#"					"specifier": "1.0.0","#
            r#"					"with": {"#
            r#"						"react": "18.0.0","#
            "					},"
            "				},"
            "			},"
            "		},"
            ""
            r#"		"packages/a": {"#
            r#"			"dependencies": {"#
            r#"				"child": {"#
            r#"					"specifier": "^1.0.0","#
            r#"					"version": "1.0.0","#
            r#"					"with": {"#
            r#"						"react": "18.0.0","#
            "					},"
            "				},"
            "			},"
            "		},"
            ""
            r#"		"packages/b": {"#
            r#"			"dependencies": {"#
            r#"				"parent": "1.0.0","#
            "			},"
            "		},"
            "	},"
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
        yaml.contains(concat!(
            "\t\t\t\t\"foo\": {\n",
            "\t\t\t\t\t\"specifier\": \"^2.0.0\",\n",
            "\t\t\t\t\t\"version\": \"2.1.0\",\n",
            "\t\t\t\t},\n",
        )),
        "{yaml}",
    );
    assert!(
        yaml.contains("\n\t\t\"foo@2.1.0\": {\n\t\t\t\"integrity\": \"sha512-AAAA\",\n\t\t},\n"),
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
    assert!(yaml.contains("\n\t\t\"injected@file:packages/injected\": {},\n"), "{yaml}");
    assert!(!yaml.contains("resolution"), "{yaml}");
    let start = yaml.find("\n\t\"settings\"").expect("a settings section");
    let end = yaml.find("\n\t\"packages\"").expect("a packages section");
    assert_eq!(
        &yaml[start..end],
        text_block_macros::text_block_fnl! {
            ""
            r#"	"settings": {"#
            r#"		"catalogs": {"#
            r#"			"default": {"#
            r#"				"@scope/renamed": "npm:@scope/real@1.0.0","#
            r#"				"pinned": "3.0.0","#
            r#"				"tool": {"#
            r#"					"specifier": "^2.0.0","#
            r#"					"version": "2.1.0","#
            "				},"
            "			},"
            "		},"
            "	},"
            ""
            r#"	"projects": {"#
            r#"		".": {"#
            r#"			"dependencies": {"#
            r#"				"@scope/renamed": "catalog:","#
            r#"				"exact": "1.2.3","#
            r#"				"injected": "workspace:*","#
            r#"				"injected-alias": "workspace:injected@*","#
            r#"				"pinned": "catalog:","#
            r#"				"ranged": {"#
            r#"					"specifier": "^1.0.0","#
            r#"					"version": "1.2.3","#
            "				},"
            r#"				"shared": {"#
            r#"					"specifier": "workspace:*","#
            r#"					"version": "link:../shared","#
            "				},"
            r#"				"tool": "catalog:","#
            "			},"
            "		},"
            ""
            r#"		"packages/app": {"#
            r#"			"dependencies": {"#
            r#"				"lib": {"#
            r#"					"specifier": "file:../lib","#
            r#"					"version": "file:packages/lib","#
            "				},"
            r#"				"pinned-link": {"#
            r#"					"specifier": "1.0.0","#
            r#"					"version": "link:../pinned-link","#
            "				},"
            "			},"
            "		},"
            "	},"
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
    let start = yaml.find("\n\t\"projects\"").expect("an importers section");
    assert_eq!(
        &yaml[start..],
        text_block_macros::text_block_fnl! {
            ""
            r#"	"projects": {"#
            r#"		".": {"#
            r#"			"devDependencies": {"#
            r#"				"@scope/a": "workspace:*","#
            "			},"
            "		},"
            ""
            r#"		"packages/a": {"#
            r#"			"name": "@scope/a","#
            "		},"
            ""
            r#"		"packages/b": {"#
            r#"			"dependencies": {"#
            r#"				"@scope/a": "workspace:^","#
            r#"				"a-alias": "workspace:@scope/a@*","#
            r#"				"tool": {"#
            r#"					"specifier": "workspace:*","#
            r#"					"version": "link:../tool","#
            "				},"
            "			},"
            "		},"
            "	},"
            "}"
        },
    );
}

#[test]
fn json_reader_allows_a_trailing_comma_and_keeps_one_in_a_string() {
    let value = crate::json_document::from_json_str::<serde_json::Value>(
        text_block_macros::text_block_fnl! {
            "{"
            r#"	"a": "x,]","#
            r#"	"b": [1, 2,],"#
            r#"	"c": {"d": "\",}",},"#
            "}"
        },
    )
    .expect("parse");
    assert_eq!(value, serde_json::json!({"a": "x,]", "b": [1, 2], "c": {"d": r#"",}"#}}));
}

/// An empty `snapshots` list means the entry's own fields are its one
/// snapshot, as a missing list does.
#[test]
fn an_empty_snapshots_list_is_the_one_snapshot() {
    let text = r#"{"lockfileVersion":"13.0","projects":{".":{"dependencies":{"a":"1.0.0"}}},"packages":{"a@1.0.0":{"integrity":"sha512-AAAA","snapshots":[]}}}"#;
    let lockfile = parse(text);
    let snapshots = lockfile.snapshots.as_ref().expect("snapshots");
    assert_eq!(snapshots.len(), 1);
    assert!(snapshots.contains_key(&"a@1.0.0".parse().expect("key")));
}

/// The version line is read by YAML's rules: the key may be quoted and a
/// comment may follow the value.
#[test]
fn the_yaml_version_line_may_be_quoted_or_commented() {
    use super::parse::declares_next_format;
    assert!(declares_next_format("lockfileVersion: '13.0' # experimental\n\nprojects: {}\n"));
    assert!(declares_next_format("'lockfileVersion': '13.0'\n"));
    assert!(declares_next_format("lockfileVersion: '13.0'\t# a tab before the comment\n"));
    assert!(declares_next_format(r#""lockfileVersion": "13.0""#));
    assert!(!declares_next_format("lockfileVersion: '9.0' # 13.0 later\n"));
    assert!(!declares_next_format("settings:\n  lockfileVersion: '13.0'\n"));
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
        yaml.contains("\n\t\t\"a@1.0.0\": {\n\t\t\t\"integrity\": \"sha512-AAAA\",\n\t\t},\n"),
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

#[test]
fn packages_named_after_metadata_keys_still_render_across_lines() {
    let text = text_block_macros::text_block! {
        "lockfileVersion: '9.0'"
        ""
        "catalogs:"
        "  default:"
        "    engines:"
        "      specifier: ^1.0.0"
        "      version: 1.0.0"
        ""
        "importers:"
        ""
        "  .:"
        "    dependencies:"
        "      os:"
        "        specifier: ^1.0.0"
        "        version: 1.0.0(resolution@1.0.0)"
        "      resolution:"
        "        specifier: ^1.0.0"
        "        version: 1.0.0"
        ""
        "packages:"
        ""
        "  engines@1.0.0:"
        "    resolution: {integrity: sha512-AAAA}"
        ""
        "  os@1.0.0:"
        "    resolution: {integrity: sha512-BBBB}"
        "    engines: {node: '>=18'}"
        "    peerDependencies:"
        "      resolution: ^1.0.0"
        ""
        "  resolution@1.0.0:"
        "    resolution: {integrity: sha512-CCCC}"
        ""
        "snapshots:"
        ""
        "  engines@1.0.0: {}"
        ""
        "  os@1.0.0(resolution@1.0.0):"
        "    dependencies:"
        "      resolution: 1.0.0"
        ""
        "  resolution@1.0.0: {}"
    };
    let (yaml, _) = written_and_read(&parse(text));
    eprintln!("yaml:\n{yaml}");
    assert!(
        yaml.contains("\t\t\t\t\"engines\": {\n\t\t\t\t\t\"specifier\": \"^1.0.0\",\n\t\t\t\t\t\"version\": \"1.0.0\",\n\t\t\t\t},\n"),
        "{yaml}",
    );
    assert!(
        yaml.contains("\t\t\t\t\"os\": {\n\t\t\t\t\t\"specifier\": \"^1.0.0\",\n\t\t\t\t\t\"version\": \"1.0.0\",\n\t\t\t\t},\n"),
        "{yaml}",
    );
    assert!(
        yaml.contains("\t\t\t\t\"resolution\": {\n\t\t\t\t\t\"specifier\": \"^1.0.0\",\n\t\t\t\t\t\"version\": \"1.0.0\",\n\t\t\t\t},\n"),
        "{yaml}",
    );
    assert!(yaml.contains("\t\t\t\"engines\": {\"node\": \">=18\"},\n"), "{yaml}");
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
        let Some((text, lockfile)) = corpus_input(path) else { continue };
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

/// A corpus lockfile in today's format, parsed. `None` for a file that
/// cannot be read or parsed, or that is already in the next format.
fn corpus_input(path: &Path) -> Option<(String, Lockfile)> {
    let text = std::fs::read_to_string(path).ok()?;
    if super::parse::declares_next_format(&crate::extract_main_document(&text)) {
        return None;
    }
    let lockfile = Lockfile::parse(&text, path).ok()??;
    Some((text, lockfile))
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
                    specifiers: None,
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

#[test]
fn directory_specifiers_survive_next_format_round_trip() {
    for specifiers in [
        serde_json::json!({}),
        serde_json::json!({ "manifestSource": "path" }),
        serde_json::json!({ "manifestSource": "workspace" }),
        serde_json::json!({
            "dependencies": { "host": "workspace:../host", "registry": "catalog:" },
            "optionalDependencies": { "extra": "npm:alias@^1" },
            "dependenciesMeta": { "host": { "injected": true } }
        }),
    ] {
        let lockfile = parse(&format!(
            "lockfileVersion: '9.0'\nimporters: {{}}\npackages:\n  local@file:../local:\n    resolution: {{type: directory, directory: ../local}}\n    specifiers: {specifiers}\nsnapshots:\n  local@file:../local: {{}}\n",
        ));
        let (written, restored) = written_and_read(&lockfile);
        assert!(written.contains(r#""specifiers""#), "{written}");
        let metadata = restored.packages
            .expect("package entries")
            .remove(&"local@file:../local".parse().expect("package key"))
            .expect("directory entry");
        assert_eq!(
            serde_json::to_value(metadata.specifiers).expect("serialize specifiers"),
            specifiers,
        );
    }
}

#[test]
fn a_directory_without_recorded_inputs_keeps_the_legacy_fallback() {
    let lockfile = parse(
        "lockfileVersion: '9.0'\nimporters: {}\npackages:\n  local@file:local:\n    resolution: {type: directory, directory: local}\nsnapshots:\n  local@file:local: {}\n",
    );
    let (written, restored) = written_and_read(&lockfile);
    assert!(!written.contains("specifiers"), "{written}");
    let packages = restored.packages.expect("package entries");
    assert_eq!(
        packages
            .values()
            .next()
            .expect("directory metadata")
            .specifiers,
        None,
    );
}

#[test]
fn converting_a_recorded_directory_to_v9_omits_the_new_fields() {
    let mut lockfile = parse(
        "lockfileVersion: '9.0'\nimporters: {}\npackages:\n  local@file:local:\n    resolution: {type: directory, directory: local}\n    specifiers: {dependencies: {child: '^1'}}\nsnapshots:\n  local@file:local: {}\n",
    );
    let (_, restored) = written_and_read(&lockfile);
    lockfile = restored;
    lockfile.lockfile_version = crate::LockfileFormat::V9.version();
    let written = lockfile.to_yaml_string().expect("write v9 lockfile");
    assert!(!written.contains("specifiers"), "{written}");
}

#[test]
fn nested_peer_references_round_trip_within_the_depth_limit() {
    for depth in [1, 8, 32] {
        written_and_read(&peer_variant_chain(depth));
    }
}

#[test]
fn overly_deep_peer_references_return_a_format_error() {
    let lockfile = as_next(peer_variant_chain(40));
    let error = lockfile
        .to_yaml_string()
        .unwrap_err()
        .to_string();
    assert!(error.contains("32 nested references"), "{error}");
}

#[test]
fn round_trip_guard_rejects_changed_transitive_peer_names() {
    let lockfile = parse(OPTIONAL_PEER_LOCKFILE);
    let mut restored = super::RestoredSections {
        importers: lockfile.importers.clone(),
        snapshots: lockfile.snapshots.clone(),
    };
    restored.snapshots
        .as_mut()
        .unwrap()
        .get_mut(&"container@3.2.2(buffer-impl@3.7.1)".parse().unwrap())
        .unwrap()
        .transitive_peer_dependencies = Some(vec!["buffer-impl".to_string()]);
    assert!(super::write::check_round_trip(&lockfile, &restored).is_err());
}

fn peer_variant_chain(depth: usize) -> Lockfile {
    let mut lockfile = parse(OPTIONAL_PEER_LOCKFILE);
    let metadata =
        lockfile.packages.as_ref().unwrap()[&"buffer-impl@3.7.1".parse().unwrap()].clone();
    lockfile.importers.clear();
    let packages = lockfile.packages.as_mut().unwrap();
    let snapshots = lockfile.snapshots.as_mut().unwrap();
    packages.clear();
    snapshots.clear();
    for major in [1, 2] {
        let mut provider: crate::PackageKey = format!("leaf@{major}.0.0").parse().unwrap();
        packages.insert(provider.clone(), metadata.clone());
        snapshots.insert(provider.clone(), crate::SnapshotEntry::default());
        for level in (0..depth).rev() {
            let key: crate::PackageKey =
                format!("layer-{level}@1.0.0({provider})").parse().unwrap();
            packages.insert(key.without_peer(), metadata.clone());
            snapshots.insert(
                key.clone(),
                crate::SnapshotEntry {
                    dependencies: Some(std::collections::HashMap::from([(
                        provider.name.clone(),
                        crate::SnapshotDepRef::Plain(provider.suffix.clone()),
                    )])),
                    ..Default::default()
                },
            );
            provider = key;
        }
    }
    lockfile
}

#[test]
fn peer_cycle_variants_use_consistent_nested_discriminators() {
    let mut lockfile = peer_variant_chain(0);
    let metadata = lockfile.packages
        .as_ref()
        .unwrap()
        .values()
        .next()
        .unwrap()
        .clone();
    for major in [1, 2] {
        for (name, other) in [("alpha", "beta"), ("beta", "alpha")] {
            let key: crate::PackageKey =
                format!("{name}@1.0.0({other}@1.0.0)(leaf@{major}.0.0)").parse().unwrap();
            let provider: crate::PackageKey =
                format!("{other}@1.0.0({name}@1.0.0)(leaf@{major}.0.0)").parse().unwrap();
            lockfile.packages
                .as_mut()
                .unwrap()
                .insert(key.without_peer(), metadata.clone());
            lockfile.snapshots
                .as_mut()
                .unwrap()
                .insert(
                    key.clone(),
                    crate::SnapshotEntry {
                        dependencies: Some(std::collections::HashMap::from([
                            (other.parse().unwrap(), crate::SnapshotDepRef::Plain(provider.suffix)),
                            ("leaf".parse().unwrap(), format!("{major}.0.0").parse().unwrap()),
                        ])),
                        ..Default::default()
                    },
                );
            if name == "alpha" {
                lockfile.importers.insert(
                    format!("project-{major}"),
                    crate::ProjectSnapshot {
                        dependencies: Some(std::collections::HashMap::from([(
                            name.parse().unwrap(),
                            crate::ResolvedDependencySpec {
                                specifier: "1.0.0".to_string(),
                                version: crate::ImporterDepVersion::Regular(key.suffix),
                            },
                        )])),
                        ..Default::default()
                    },
                );
            }
        }
    }
    written_and_read(&lockfile);
}

#[test]
fn hashed_peer_keeps_an_ordinary_alias_as_a_dependency() {
    for (peer_alias, ordinary_alias) in [("provider", "renamed"), ("renamed", "provider")] {
        let suffix =
            crate::PeerBindings::Explicit(vec!["provider@1.0.0".to_string()]).to_suffix(3).unwrap();
        let lockfile = parse(&format!(
            r"lockfileVersion: '9.0'
settings:
  autoInstallPeers: false
  excludeLinksFromLockfile: false
  peersSuffixMaxLength: 3
importers:
  .:
    dependencies:
      consumer:
        specifier: 1.0.0
        version: 1.0.0{suffix}
packages:
  consumer@1.0.0:
    resolution: {{integrity: sha512-AAAA}}
    peerDependencies:
      {peer_alias}: '*'
  provider@1.0.0:
    resolution: {{integrity: sha512-BBBB}}
snapshots:
  consumer@1.0.0{suffix}:
    dependencies:
      {peer_alias}: provider@1.0.0
      {ordinary_alias}: provider@1.0.0
  provider@1.0.0: {{}}
",
        ));
        written_and_read(&lockfile);
    }
}

#[test]
fn hashed_transitive_peer_keeps_a_link_provider() {
    let suffix = crate::PeerBindings::Explicit(vec!["provider@packages+provider".to_string()])
        .to_suffix(3)
        .unwrap();
    let lockfile = parse(&format!(
        r"lockfileVersion: '9.0'
settings:
  autoInstallPeers: false
  excludeLinksFromLockfile: false
  peersSuffixMaxLength: 3
importers:
  .:
    dependencies:
      parent:
        specifier: 1.0.0
        version: 1.0.0{suffix}
packages:
  parent@1.0.0:
    resolution: {{integrity: sha512-AAAA}}
  child@1.0.0:
    resolution: {{integrity: sha512-BBBB}}
    peerDependencies:
      provider: '*'
snapshots:
  parent@1.0.0{suffix}:
    dependencies:
      child: 1.0.0{suffix}
    transitivePeerDependencies:
      - provider
  child@1.0.0{suffix}:
    dependencies:
      provider: link:packages/provider
",
    ));
    written_and_read(&lockfile);
}

#[test]
fn hashed_transitive_peer_selects_the_bound_branch() {
    for other_reference in ["2.0.0", "link:packages/provider"] {
        let suffix =
            crate::PeerBindings::Explicit(vec!["provider@1.0.0".to_string()]).to_suffix(3).unwrap();
        let other_version = if other_reference.starts_with("link:") {
            "packages+provider"
        } else {
            other_reference
        };
        let other_suffix = crate::PeerBindings::Explicit(vec![format!("provider@{other_version}")])
            .to_suffix(3)
            .unwrap();
        let mut lockfile = parse(&format!(
            r"lockfileVersion: '9.0'
settings:
  autoInstallPeers: false
  excludeLinksFromLockfile: false
  peersSuffixMaxLength: 3
importers:
  .:
    dependencies:
      parent:
        specifier: 1.0.0
        version: 1.0.0{suffix}
packages:
  parent@1.0.0:
    resolution: {{integrity: sha512-AAAA}}
  first@1.0.0:
    resolution: {{integrity: sha512-BBBB}}
    peerDependencies:
      provider: '*'
  second@1.0.0:
    resolution: {{integrity: sha512-CCCC}}
    peerDependencies:
      provider: '*'
  provider@1.0.0:
    resolution: {{integrity: sha512-DDDD}}
  provider@2.0.0:
    resolution: {{integrity: sha512-EEEE}}
snapshots:
  parent@1.0.0{suffix}:
    dependencies:
      first: 1.0.0{suffix}
      second: 1.0.0{other_suffix}
    transitivePeerDependencies:
      - provider
  first@1.0.0{suffix}:
    dependencies:
      provider: 1.0.0
  second@1.0.0{other_suffix}:
    dependencies:
      provider: {other_reference}
  provider@1.0.0: {{}}
  provider@2.0.0: {{}}
",
        ));
        let parent_key = format!("parent@1.0.0{suffix}").parse().unwrap();
        let unbound_branch = format!("second@1.0.0{other_suffix}").parse().unwrap();
        let bound_branch = format!("first@1.0.0{suffix}").parse().unwrap();
        let branches = lockfile.snapshots
            .as_mut()
            .unwrap()
            .get_mut(&parent_key)
            .unwrap()
            .dependencies
            .as_mut()
            .unwrap();
        for (reference, target) in branches
            .values_mut()
            .zip([unbound_branch, bound_branch])
        {
            *reference = crate::SnapshotDepRef::Alias(target);
        }
        written_and_read(&lockfile);
    }
}

#[test]
fn hashed_transitive_peer_distinguishes_provider_peer_bindings() {
    let suffix = |segments| crate::PeerBindings::Explicit(segments).to_suffix(3).unwrap();
    let first_provider = suffix(vec!["nested@1.0.0".to_string()]);
    let second_provider = suffix(vec!["nested@2.0.0".to_string()]);
    let first = suffix(vec![format!("provider@1.0.0{first_provider}")]);
    let second = suffix(vec![format!("provider@1.0.0{second_provider}")]);
    let mut lockfile = parse(&format!(
        r"lockfileVersion: '9.0'
settings:
  autoInstallPeers: false
  excludeLinksFromLockfile: false
  peersSuffixMaxLength: 3
importers:
  .:
    dependencies:
      parent:
        specifier: 1.0.0
        version: 1.0.0{first}
packages:
  parent@1.0.0:
    resolution: {{integrity: sha512-AAAA}}
  child@1.0.0:
    resolution: {{integrity: sha512-BBBB}}
    peerDependencies:
      provider: '*'
  provider@1.0.0:
    resolution: {{integrity: sha512-CCCC}}
    peerDependencies:
      nested: '*'
  nested@1.0.0:
    resolution: {{integrity: sha512-DDDD}}
  nested@2.0.0:
    resolution: {{integrity: sha512-EEEE}}
snapshots:
  parent@1.0.0{first}:
    dependencies:
      first: child@1.0.0{first}
      second: child@1.0.0{second}
    transitivePeerDependencies: [provider]
  child@1.0.0{first}:
    dependencies:
      provider: 1.0.0{first_provider}
  child@1.0.0{second}:
    dependencies:
      provider: 1.0.0{second_provider}
  provider@1.0.0{first_provider}:
    dependencies:
      nested: 1.0.0
  provider@1.0.0{second_provider}:
    dependencies:
      nested: 2.0.0
  nested@1.0.0: {{}}
  nested@2.0.0: {{}}
",
    ));
    let parent = format!("parent@1.0.0{first}").parse().unwrap();
    let branches = lockfile.snapshots
        .as_mut()
        .unwrap()
        .get_mut(&parent)
        .unwrap()
        .dependencies
        .as_mut()
        .unwrap();
    for (reference, target) in branches
        .values_mut()
        .zip([second, first])
    {
        *reference = crate::SnapshotDepRef::Alias(format!("child@1.0.0{target}").parse().unwrap());
    }
    written_and_read(&lockfile);
}

#[test]
fn mixed_hashed_peer_groups_with_many_providers_round_trip() {
    let hashed = |segment| crate::PeerBindings::Explicit(vec![segment]).to_suffix(3).unwrap();
    let nested = hashed("nested@1.0.0".to_string());
    let cycle = hashed("root@1.0.0".to_string());
    let groups = (0..8)
        .map(|index| {
            if index == 7 {
                format!("provider-{index}@1.0.0")
            } else {
                format!("provider-{index}@1.0.0{nested}")
            }
        })
        .collect();
    let suffix = crate::PeerBindings::Explicit(groups).to_suffix(3).unwrap();
    let mut declarations = String::new();
    let mut dependencies = String::new();
    let mut packages = String::new();
    let mut snapshots = String::new();
    for index in 0..8 {
        let (peer, version, provider_suffix) = if index == 7 {
            ("root", format!("1.0.0{suffix}"), &cycle)
        } else {
            ("nested", "1.0.0".to_string(), &nested)
        };
        writeln!(declarations, "      provider-{index}: '*'").unwrap();
        writeln!(dependencies, "      provider-{index}: 1.0.0{provider_suffix}").unwrap();
        write!(
            packages,
            r"  provider-{index}@1.0.0:
    resolution: {{integrity: sha512-BBBB}}
    peerDependencies:
      {peer}: '*'
",
        )
        .unwrap();
        write!(
            snapshots,
            r"  provider-{index}@1.0.0{provider_suffix}:
    dependencies:
      {peer}: {version}
",
        )
        .unwrap();
    }
    let lockfile = parse(&format!(
        r"lockfileVersion: '9.0'
settings:
  autoInstallPeers: false
  excludeLinksFromLockfile: false
  peersSuffixMaxLength: 3
importers:
  .:
    dependencies:
      root:
        specifier: 1.0.0
        version: 1.0.0{suffix}
packages:
  root@1.0.0:
    resolution: {{integrity: sha512-AAAA}}
    peerDependencies:
{declarations}      own: '*'
{packages}  own@1.0.0:
    resolution: {{integrity: sha512-CCCC}}
  nested@1.0.0:
    resolution: {{integrity: sha512-DDDD}}
snapshots:
  root@1.0.0{suffix}:
    dependencies:
{dependencies}      own: 1.0.0
{snapshots}  own@1.0.0: {{}}
  nested@1.0.0: {{}}
",
    ));
    written_and_read(&lockfile);
}

#[test]
fn recorded_peers_do_not_require_hashed_subset_recovery() {
    for bound_count in 0..=14 {
        let lockfile = lockfile_with_recorded_peers(bound_count);
        let key = lockfile.snapshots
            .as_ref()
            .unwrap()
            .keys()
            .find(|key| key.name.to_string() == "consumer")
            .unwrap();
        assert_eq!(crate::BoundProviders::from_key(key).is_hashed(), bound_count != 0);
        let bindings = crate::BoundProviders::of_snapshot((&lockfile).into(), key);
        assert!(!bindings.is_hashed());
        for index in 0..14 {
            let provider = format!("provider-{index:02}@1.0.0").parse().unwrap();
            assert_eq!(bindings.binds(&provider), index < bound_count);
        }
        written_and_read(&lockfile);
    }
}

fn lockfile_with_recorded_peers(bound_count: usize) -> Lockfile {
    let mut peers = serde_json::Map::new();
    let mut dependencies = serde_json::Map::new();
    let mut packages = serde_json::Map::new();
    let mut root_dependencies =
        serde_json::Map::from_iter([("consumer".to_string(), serde_json::json!("1.0.0"))]);
    for index in 0..14 {
        let name = format!("provider-{index:02}");
        packages.insert(
            format!("{name}@1.0.0"),
            serde_json::json!({
                "resolution": {"tarball": format!("https://example.org/{name}.tgz")},
            }),
        );
        if index < bound_count {
            root_dependencies.insert(name.clone(), serde_json::json!("1.0.0"));
            peers.insert(name, serde_json::json!({"specifier": "*", "version": "1.0.0"}));
        } else {
            peers.insert(name.clone(), serde_json::json!("*"));
            dependencies.insert(name, serde_json::json!("1.0.0"));
        }
    }
    packages.insert(
        "consumer@1.0.0".to_string(),
        serde_json::json!({
            "resolution": {"tarball": "https://example.org/consumer.tgz"},
            "peerDependencies": peers, "dependencies": dependencies,
        }),
    );
    let document = serde_json::json!({
        "lockfileVersion": "13.0",
        "settings": {"autoInstallPeers": false, "excludeLinksFromLockfile": false, "peersSuffixMaxLength": 3},
        "projects": {".": {"dependencies": root_dependencies}},
        "packages": packages,
    });
    parse(&document.to_string())
}
