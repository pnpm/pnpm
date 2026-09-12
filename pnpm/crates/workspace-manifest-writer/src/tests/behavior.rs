use super::{
    TempDir, UpdateWorkspaceManifestOptions, WORKSPACE_MANIFEST_FILENAME, catalogs, fs, overrides,
    run, run_age_excludes, run_allow_builds, run_allow_builds_clearing_legacy, run_overrides,
    run_prune_allow_builds, run_remove_overrides, run_scaffold_allow_builds, run_update_field,
    run_with,
};

#[test]
fn preserves_quotes_and_appends_new_entry() {
    let original = "catalog:\n  \"bar\": \"2.0.0\"\n  'foo': '1.0.0'\n  qar: 3.0.0\n";
    let out = run(
        Some(original),
        &catalogs(&[(
            "default",
            &[("foo", "1.0.0"), ("bar", "2.0.0"), ("qar", "3.0.0"), ("zoo", "4.0.0")],
        )]),
    )
    .expect("written");
    assert_eq!(
        out,
        "catalog:\n  \"bar\": \"2.0.0\"\n  'foo': '1.0.0'\n  qar: 3.0.0\n  zoo: 4.0.0\n",
    );
}

#[test]
fn no_blank_lines_when_original_has_none() {
    let original = "packages:\n  - '*'\nallowBuilds:\n  foo: true\n";
    let out = run(Some(original), &catalogs(&[("default", &[("bar", "2.0.0")])])).expect("written");
    assert_eq!(out, "packages:\n  - '*'\nallowBuilds:\n  foo: true\ncatalog:\n  bar: 2.0.0\n");
}

#[test]
fn inserts_entry_in_sorted_position() {
    let original = "catalog:\n  apple: '1.0.0'\n  mango: '2.0.0'\n  zebra: '3.0.0'\n";
    let out =
        run(Some(original), &catalogs(&[("default", &[("banana", "4.0.0")])])).expect("written");
    assert_eq!(
        out,
        "catalog:\n  apple: '1.0.0'\n  banana: 4.0.0\n  mango: '2.0.0'\n  zebra: '3.0.0'\n",
    );
}

#[test]
fn appends_entry_when_block_is_unordered() {
    let original = "catalog:\n  zebra: '1.0.0'\n  apple: '2.0.0'\n";
    let out =
        run(Some(original), &catalogs(&[("default", &[("mango", "3.0.0")])])).expect("written");
    assert_eq!(out, "catalog:\n  zebra: '1.0.0'\n  apple: '2.0.0'\n  mango: 3.0.0\n");
}

#[test]
fn no_op_when_entry_already_present_with_same_specifier() {
    let original = "catalog:\n  # keep this comment\n  foo: ^1.0.0\n";
    let out =
        run(Some(original), &catalogs(&[("default", &[("foo", "^1.0.0")])])).expect("written");
    assert_eq!(out, original);
}

#[test]
fn inserts_entry_into_a_four_space_indented_block() {
    let original = "catalogs:\n    react:\n        react: 18.0.0\n";
    let out =
        run(Some(original), &catalogs(&[("react", &[("react-dom", "18.0.0")])])).expect("written");
    assert_eq!(out, "catalogs:\n    react:\n        react: 18.0.0\n        react-dom: 18.0.0\n");
}

#[test]
fn quotes_scoped_package_keys() {
    // A key starting with `@` cannot be a YAML plain scalar, so it must be
    // quoted — both when creating the block and when adding an entry.
    let out = run(None, &catalogs(&[("default", &[("@pnpm.e2e/foo", "1.0.0")])])).expect("written");
    assert_eq!(out, "catalog:\n  '@pnpm.e2e/foo': 1.0.0\n");

    let out =
        run(Some(&out), &catalogs(&[("default", &[("@pnpm.e2e/bar", "2.0.0")])])).expect("written");
    assert_eq!(out, "catalog:\n  '@pnpm.e2e/bar': 2.0.0\n  '@pnpm.e2e/foo': 1.0.0\n");
}

#[test]
fn preserves_comment_when_inserting_before_commented_entry() {
    let original = "catalog:\n  apple: 1.0.0\n  # note about zebra\n  zebra: 3.0.0\n";
    let out =
        run(Some(original), &catalogs(&[("default", &[("mango", "2.0.0")])])).expect("written");
    assert_eq!(
        out,
        "catalog:\n  apple: 1.0.0\n  mango: 2.0.0\n  # note about zebra\n  zebra: 3.0.0\n",
    );
}

#[test]
fn allow_builds_creates_block_when_absent() {
    let out = run_allow_builds(None, &[("esbuild", true)]);
    assert_eq!(out.as_deref(), Some("allowBuilds:\n  esbuild: true\n"));
}

#[test]
fn allow_builds_writes_boolean_values_unquoted() {
    let out = run_allow_builds(None, &[("esbuild", true), ("@scope/pkg", false)]);
    assert_eq!(out.as_deref(), Some("allowBuilds:\n  '@scope/pkg': false\n  esbuild: true\n"));
}

#[test]
fn allow_builds_upserts_existing_entry() {
    let original = "allowBuilds:\n  esbuild: false\n";
    let out = run_allow_builds(Some(original), &[("esbuild", true)]);
    assert_eq!(out.as_deref(), Some("allowBuilds:\n  esbuild: true\n"));
}

#[test]
fn scaffold_allow_builds_creates_block_when_absent() {
    let out = run_scaffold_allow_builds(Some("packages: []\n"), &["es5-ext"]);
    assert_eq!(
        out.as_deref(),
        Some("packages: []\nallowBuilds:\n  es5-ext: set this to true or false\n"),
    );
}

#[test]
fn scaffold_allow_builds_leaves_a_decided_entry_alone() {
    let original = "allowBuilds:\n  esbuild: false\n";
    let out = run_scaffold_allow_builds(Some(original), &["esbuild", "es5-ext"]);
    assert_eq!(
        out.as_deref(),
        Some("allowBuilds:\n  es5-ext: set this to true or false\n  esbuild: false\n"),
    );
}

#[test]
fn allow_builds_preserves_other_keys_and_comments() {
    let original = "# top comment\nstoreDir: ../store\n";
    let out = run_allow_builds(Some(original), &[("esbuild", true)]).expect("file written");
    assert!(out.contains("# top comment"), "comment preserved");
    assert!(out.contains("storeDir: ../store"), "existing key preserved");
    assert!(out.contains("allowBuilds:\n  esbuild: true"), "block appended");
}

#[test]
fn allow_builds_upserts_a_key_containing_a_colon() {
    // Artifact allow-build keys keep the full pkgId, which contains `:`
    // (e.g. a tarball/git URL). The upsert must find and toggle the
    // existing entry instead of appending a duplicate — which a
    // first-colon line scan would do by truncating the key.
    let key = "foo@https://example.com/foo.tgz";
    let original = format!("allowBuilds:\n  '{key}': false\n");
    let out = run_allow_builds(Some(&original), &[(key, true)]).expect("file written");
    assert_eq!(out, format!("allowBuilds:\n  '{key}': true\n"));
    assert_eq!(out.matches(key).count(), 1, "exactly one entry, no duplicate: {out}");
}

#[test]
fn allow_builds_creates_and_round_trips_a_colon_key() {
    let key = "foo@https://example.com/foo.tgz";
    let created = run_allow_builds(None, &[(key, true)]).expect("file written");
    assert!(created.contains(key), "key written verbatim: {created}");
    // Re-upserting the same value is a no-op (the entry is found, not duplicated).
    let same = run_allow_builds(Some(&created), &[(key, true)]);
    assert_eq!(same.as_deref(), Some(created.as_str()), "idempotent: {created}");
    // Toggling flips the existing entry rather than appending a duplicate.
    let toggled = run_allow_builds(Some(&created), &[(key, false)]).expect("written");
    assert_eq!(toggled.matches(key).count(), 1, "no duplicate after toggle: {toggled}");
}

#[test]
fn overrides_block_is_created() {
    let out = run_overrides(None, &overrides(&[("foo@<1.0.1", "^1.0.1")])).expect("written");
    assert_eq!(out, "overrides:\n  foo@<1.0.1: ^1.0.1\n");
}

#[test]
fn overrides_quote_keys_and_values_that_need_it() {
    let out =
        run_overrides(None, &overrides(&[("@scope/foo@>=1.0.0", ">=1.0.1")])).expect("written");
    assert_eq!(out, "overrides:\n  '@scope/foo@>=1.0.0': '>=1.0.1'\n");
}

#[test]
fn overrides_merge_into_an_existing_block() {
    let original = "overrides:\n  bar@1: 2\n";
    let out =
        run_overrides(Some(original), &overrides(&[("foo@<1.0.1", "^1.0.1")])).expect("written");
    assert_eq!(out, "overrides:\n  bar@1: 2\n  foo@<1.0.1: ^1.0.1\n");
}

#[test]
fn overrides_are_added_after_packages() {
    let original = "packages:\n  - '*'\n";
    let out =
        run_overrides(Some(original), &overrides(&[("foo@<1.0.1", "^1.0.1")])).expect("written");
    assert_eq!(out, "packages:\n  - '*'\noverrides:\n  foo@<1.0.1: ^1.0.1\n");
}

#[test]
fn overrides_noop_when_already_present() {
    let original = "overrides:\n  foo@<1.0.1: ^1.0.1\n";
    let out =
        run_overrides(Some(original), &overrides(&[("foo@<1.0.1", "^1.0.1")])).expect("written");
    assert_eq!(out, original);
}

#[test]
fn minimum_release_age_exclude_block_is_created() {
    let out = run_age_excludes(None, &["foo@1.0.0", "bar@2.0.0"]).expect("written");
    assert_eq!(out, "minimumReleaseAgeExclude:\n  - foo@1.0.0\n  - bar@2.0.0\n");
}

#[test]
fn minimum_release_age_exclude_added_after_packages() {
    let original = "packages:\n  - '*'\n";
    let out = run_age_excludes(Some(original), &["foo@1.0.0"]).expect("written");
    assert_eq!(out, "packages:\n  - '*'\nminimumReleaseAgeExclude:\n  - foo@1.0.0\n");
}

#[test]
fn minimum_release_age_exclude_replaces_existing_block() {
    let original = "minimumReleaseAgeExclude:\n  - foo@1.0.0\n";
    let out = run_age_excludes(Some(original), &["foo@1.0.0", "bar@2.0.0"]).expect("written");
    assert_eq!(out, "minimumReleaseAgeExclude:\n  - foo@1.0.0\n  - bar@2.0.0\n");
}

#[test]
fn minimum_release_age_exclude_noop_when_unchanged() {
    let original = "minimumReleaseAgeExclude:\n  - foo@1.0.0\n";
    let out = run_age_excludes(Some(original), &["foo@1.0.0"]).expect("written");
    assert_eq!(out, original);
}

#[test]
fn minimum_release_age_exclude_empty_removes_the_block() {
    let original = "packages:\n  - '*'\nminimumReleaseAgeExclude:\n  - foo@1.0.0\n";
    let out = run_age_excludes(Some(original), &[]).expect("written");
    assert_eq!(out, "packages:\n  - '*'\n");
}

#[test]
fn minimum_release_age_exclude_add_keeps_the_existing_entries_comments() {
    let added = ["new@1.0.0".to_string()];
    let out = run_with(
        Some("minimumReleaseAgeExclude:\n  - foo@1.0.0 # audited\n"),
        &UpdateWorkspaceManifestOptions {
            added_minimum_release_age_excludes: &added,
            ..Default::default()
        },
    )
    .expect("written");

    assert_eq!(out, "minimumReleaseAgeExclude:\n  - foo@1.0.0 # audited\n  - new@1.0.0\n");
}

/// A rewritten entry loses its own comment, matching the TypeScript writer's
/// node reuse.
#[test]
fn minimum_release_age_exclude_add_keeps_other_comments_when_one_entry_is_rewritten() {
    let added = ["foo@2.0.0".to_string()];
    let out = run_with(
        Some("minimumReleaseAgeExclude:\n  - foo@1.0.0 # audited\n  - bar@1.0.0 # pinned\n"),
        &UpdateWorkspaceManifestOptions {
            added_minimum_release_age_excludes: &added,
            ..Default::default()
        },
    )
    .expect("written");

    assert_eq!(out, "minimumReleaseAgeExclude:\n  - foo@1.0.0 || 2.0.0\n  - bar@1.0.0 # pinned\n");
}

#[test]
fn minimum_release_age_exclude_add_ends_a_reused_last_line_that_has_no_newline() {
    let added = ["new@1.0.0".to_string()];
    let out = run_with(
        Some("minimumReleaseAgeExclude:\n  - foo@1.0.0"),
        &UpdateWorkspaceManifestOptions {
            added_minimum_release_age_excludes: &added,
            ..Default::default()
        },
    )
    .expect("written");

    assert_eq!(out, "minimumReleaseAgeExclude:\n  - foo@1.0.0\n  - new@1.0.0\n");
}

#[test]
fn minimum_release_age_exclude_add_matches_the_blocks_crlf_line_endings() {
    let added = ["new@1.0.0".to_string()];
    let out = run_with(
        Some("minimumReleaseAgeExclude:\r\n  - foo@1.0.0\r\n"),
        &UpdateWorkspaceManifestOptions {
            added_minimum_release_age_excludes: &added,
            ..Default::default()
        },
    )
    .expect("written");

    assert_eq!(out, "minimumReleaseAgeExclude:\r\n  - foo@1.0.0\r\n  - new@1.0.0\r\n");
}

#[test]
fn set_overrides_refuses_to_clobber_a_non_scalar_value() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    // A hand-written parent-scoped (object) override at the same selector key.
    fs::write(&path, "overrides:\n  foo@<2.0.0:\n    bar: 1.0.0\n").expect("seed manifest");

    let err = crate::set_overrides(dir.path(), [("foo@<2.0.0", "^2.0.0")])
        .expect_err("must refuse to overwrite a non-scalar override");

    assert!(matches!(err, crate::UpdateWorkspaceManifestError::OverrideConflict { .. }));
    // The original object value is left untouched.
    let after = fs::read_to_string(&path).expect("read manifest");
    assert_eq!(after, "overrides:\n  foo@<2.0.0:\n    bar: 1.0.0\n");
}

#[test]
fn set_overrides_edits_an_inline_flow_block() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    fs::write(&path, "overrides: { foo: 1.0.0 } # pinned\n").expect("seed manifest");

    crate::set_overrides(dir.path(), [("bar", "^2.0.0")]).expect("set_overrides succeeds");

    let after = fs::read_to_string(&path).expect("read manifest");
    assert_eq!(after, "overrides: { bar: ^2.0.0, foo: 1.0.0 } # pinned\n");
}

#[test]
fn set_overrides_updates_an_entry_of_an_inline_flow_block() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    fs::write(&path, "overrides: { 'foo': \"1.0.0\", bar: 2.0.0 }\n").expect("seed manifest");

    crate::set_overrides(dir.path(), [("foo", "^3.0.0")]).expect("set_overrides succeeds");

    let after = fs::read_to_string(&path).expect("read manifest");
    assert_eq!(after, "overrides: { 'foo': ^3.0.0, bar: 2.0.0 }\n");
}

#[test]
fn set_overrides_refuses_a_multiline_flow_block() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);
    let original = "overrides: {\n  foo: 1.0.0, # pinned\n}\n";
    fs::write(&path, original).expect("seed manifest");

    let err = crate::set_overrides(dir.path(), [("bar", "^2.0.0")])
        .expect_err("must refuse a multi-line inline overrides block");

    assert!(matches!(err, crate::UpdateWorkspaceManifestError::UnsupportedInlineBlock { .. }));
    let after = fs::read_to_string(&path).expect("read manifest");
    assert_eq!(after, original);
}

#[test]
fn set_allow_builds_rejects_control_characters() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join(WORKSPACE_MANIFEST_FILENAME);

    // A newline in a package name (e.g. a crafted `--allow-build`) would
    // splice into a multi-line scalar and corrupt the block.
    let err = crate::set_allow_builds(dir.path(), [("esbuild\ninjected: true", true)])
        .expect_err("must reject a control character");

    assert!(matches!(err, crate::UpdateWorkspaceManifestError::InvalidControlCharacter { .. }));
    assert!(!path.exists(), "nothing should be written");
}

#[test]
fn minimum_release_age_excludes_rejects_control_characters() {
    let dir = TempDir::new().expect("temp dir");

    let err =
        crate::set_minimum_release_age_excludes(dir.path(), &["foo\r\nbar@1.0.0".to_string()])
            .expect_err("must reject a control character");

    assert!(matches!(err, crate::UpdateWorkspaceManifestError::InvalidControlCharacter { .. }));
}

#[test]
fn set_overrides_rejects_control_characters() {
    let dir = TempDir::new().expect("temp dir");

    let err = crate::set_overrides(dir.path(), [("foo@<2.0.0\nx", "^2.0.0")])
        .expect_err("must reject a control character");

    assert!(matches!(err, crate::UpdateWorkspaceManifestError::InvalidControlCharacter { .. }));
}

#[test]
fn remove_overrides_drops_only_the_named_entry() {
    let original = "overrides:\n  foo: link:../foo\n  bar: link:../bar\n  baz: 1.0.0\n";
    let out = run_remove_overrides(Some(original), &["foo"]).expect("file kept");
    assert_eq!(out, "overrides:\n  bar: link:../bar\n  baz: 1.0.0\n");
}

#[test]
fn remove_overrides_drops_the_block_when_emptied_but_keeps_siblings() {
    let original = "packages:\n  - '*'\noverrides:\n  foo: link:../foo\n";
    let out = run_remove_overrides(Some(original), &["foo"]).expect("file kept");
    assert_eq!(out, "packages:\n  - '*'\n");
}

#[test]
fn remove_overrides_is_a_noop_for_absent_selectors() {
    let original = "overrides:\n  foo: link:../foo\n";
    let out = run_remove_overrides(Some(original), &["missing"]).expect("file kept");
    assert_eq!(out, original);
}

#[test]
fn remove_overrides_handles_flow_style_mappings() {
    let original = "overrides: { foo: link:../foo, bar: 1.0.0 }\n";
    let out = run_remove_overrides(Some(original), &["foo"]).expect("file kept");
    assert_eq!(out, "overrides: { bar: 1.0.0 }\n");
}

#[test]
fn remove_overrides_drops_a_flow_style_block_when_emptied() {
    let original = "packages:\n  - '*'\noverrides: { foo: link:../foo }\n";
    let out = run_remove_overrides(Some(original), &["foo"]).expect("file kept");
    assert_eq!(out, "packages:\n  - '*'\n");
}

#[test]
fn remove_overrides_preserves_non_string_entries_in_block_style() {
    let original = "overrides:\n  foo: link:../foo\n  bar:\n    nested: value\n";
    let out = run_remove_overrides(Some(original), &["foo"]).expect("file kept");
    assert_eq!(out, "overrides:\n  bar:\n    nested: value\n");
}

#[test]
fn remove_overrides_keeps_block_when_only_non_string_entry_remains() {
    // Removing the last string entry must not delete the block while a
    // non-string entry (which the decoded map drops) is still present.
    let original = "overrides:\n  foo: link:../foo\n  bar:\n    nested: value\n";
    let out = run_remove_overrides(Some(original), &["foo"]).expect("file kept");
    assert!(out.contains("bar:"), "non-string override must survive: {out}");
}

#[test]
fn allow_builds_clearing_legacy_drops_every_legacy_key_in_the_same_write() {
    let original = "packages:\n  - '*'\nonlyBuiltDependencies:\n  - esbuild\nonlyBuiltDependenciesFile: allowed.json\nneverBuiltDependencies:\n  - fsevents\nignoredBuiltDependencies:\n  - foo\n";
    let out =
        run_allow_builds_clearing_legacy(Some(original), &[("esbuild", true)]).expect("file kept");
    eprintln!("MANIFEST:\n{out}\n");
    assert_eq!(out, "packages:\n  - '*'\nallowBuilds:\n  esbuild: true\n");
}

#[test]
fn allow_builds_clearing_legacy_is_a_noop_when_no_legacy_key_is_present() {
    let original = "packages:\n  - '*'\nallowBuilds:\n  esbuild: true\n";
    let out = run_allow_builds_clearing_legacy(Some(original), &[]).expect("file kept");
    eprintln!("MANIFEST:\n{out}\n");
    assert_eq!(out, original);
}

#[test]
fn remove_overrides_keeps_non_string_entries_of_a_flow_style_block() {
    // The decoded map cannot reserialize the non-string `bar`, but the flow
    // splice keeps its text, so the entry survives the removal of `foo`.
    let original = "overrides: { foo: link:../foo, bar: { nested: value } }\n";
    let out = run_remove_overrides(Some(original), &["foo"]).expect("file kept");
    assert_eq!(out, "overrides: { bar: { nested: value } }\n");
}

#[test]
fn set_object_field_with_json() {
    let value = serde_json::json!({
        "@babel/parser": { "peerDependencies": { "@babel/types": "*" } },
        "jest-circus": { "dependencies": { "slash": "3" } },
    });
    let out = run_update_field(None, "packageExtensions", &value).expect("file written");
    let parsed: serde_json::Value = serde_saphyr::from_str(&out).expect("parse");
    assert_eq!(parsed["packageExtensions"], value);
}

#[test]
fn delete_last_field_leaves_no_trailing_blank_line() {
    let out = run_update_field(
        Some("cacheDir: ~/cache\n\nvirtualStoreDir: .pnpm\n"),
        "virtualStoreDir",
        &serde_json::Value::Null,
    )
    .expect("file kept");
    assert_eq!(out, "cacheDir: ~/cache\n");
}

#[test]
fn delete_last_field_keeps_a_kept_chomped_block_scalars_trailing_blank() {
    for header in [
        "|+",
        ">+",
        "|+2",
        "|2+",
        "|2+ # keep the breaks",
        "|+ # retain > blanks",
        "&notes |+",
        "!!str >+",
    ] {
        let original = format!("notes: {header}\n  foo\n\nvirtualStoreDir: .pnpm\n");
        let out = run_update_field(Some(&original), "virtualStoreDir", &serde_json::Value::Null)
            .expect("file kept");
        assert_eq!(out, format!("notes: {header}\n  foo\n\n"), "header {header}");
    }
}

#[test]
fn delete_last_field_keeps_the_blank_below_a_deeper_indented_scalar_line() {
    let out = run_update_field(
        Some("notes: |+\n  foo\n    bar\n\nvirtualStoreDir: .pnpm\n"),
        "virtualStoreDir",
        &serde_json::Value::Null,
    )
    .expect("file kept");
    assert_eq!(out, "notes: |+\n  foo\n    bar\n\n");
}

#[test]
fn delete_last_field_keeps_the_blank_of_a_scalar_under_a_quoted_key() {
    let out = run_update_field(
        Some("\"notes: title\": |+\n  foo\n\nvirtualStoreDir: .pnpm\n"),
        "virtualStoreDir",
        &serde_json::Value::Null,
    )
    .expect("file kept");
    assert_eq!(out, "\"notes: title\": |+\n  foo\n\n");
}

#[test]
fn delete_last_field_keeps_the_blank_of_a_scalar_under_an_apostrophe_key() {
    for key in ["it's", "'it''s: title'"] {
        let original = format!("{key}: |+\n  foo\n\nvirtualStoreDir: .pnpm\n");
        let out = run_update_field(Some(&original), "virtualStoreDir", &serde_json::Value::Null)
            .expect("file kept");
        assert_eq!(out, format!("{key}: |+\n  foo\n\n"), "key {key}");
    }
}

#[test]
fn delete_last_field_drops_a_separator_below_a_header_written_in_a_comment() {
    for line in ["notes: text # detail: |+", "notes: text\n# detail: |+"] {
        let original = format!("{line}\n\nvirtualStoreDir: .pnpm\n");
        let out = run_update_field(Some(&original), "virtualStoreDir", &serde_json::Value::Null)
            .expect("file kept");
        assert_eq!(out, format!("{line}\n"), "line {line}");
    }
}

#[test]
fn delete_last_field_drops_a_separator_below_a_quoted_scalar_holding_a_header() {
    let out = run_update_field(
        Some("notes: \"foo |+ #\"\n\nvirtualStoreDir: .pnpm\n"),
        "virtualStoreDir",
        &serde_json::Value::Null,
    )
    .expect("file kept");
    assert_eq!(out, "notes: \"foo |+ #\"\n");
}

#[test]
fn delete_last_field_drops_a_separator_below_an_unrelated_kept_chomped_scalar() {
    let out = run_update_field(
        Some("notes: |+\n  foo\n\nstoreDir: ~/store\n\nvirtualStoreDir: .pnpm\n"),
        "virtualStoreDir",
        &serde_json::Value::Null,
    )
    .expect("file kept");
    assert_eq!(out, "notes: |+\n  foo\n\nstoreDir: ~/store\n");
}

#[test]
fn changing_the_value_of_the_last_field_keeps_its_single_blank_line() {
    let out = run_update_field(
        Some("cacheDir: ~/cache\n\nstoreDir: ~/store\n"),
        "storeDir",
        &serde_json::json!("~/other"),
    )
    .expect("file written");
    assert_eq!(out, "cacheDir: ~/cache\n\nstoreDir: ~/other\n");
}

#[test]
fn delete_unset_field_is_noop() {
    let out = run_update_field(Some("cacheDir: ~/cache\n"), "storeDir", &serde_json::Value::Null)
        .expect("file kept");
    let parsed: indexmap::IndexMap<String, serde_json::Value> =
        serde_saphyr::from_str(&out).expect("parse");
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed["cacheDir"], serde_json::json!("~/cache"));
}

/// pnpm scaffolds undecided entries with a multi-word plain scalar.
/// Deciding one replaces the whole value: ending it at the first
/// whitespace would leave `true this to true or false` behind, which
/// YAML reads as a string, so the package would stay undecided.
#[test]
fn allow_builds_replaces_a_multi_word_placeholder_value() {
    let out = run_allow_builds(
        Some("allowBuilds:\n  esbuild: set this to true or false\n"),
        &[("esbuild", true)],
    );
    assert_eq!(out.as_deref(), Some("allowBuilds:\n  esbuild: true\n"));
}

/// A comment after the value is the one thing that must survive the
/// replacement, which is why the value span stops at ` #`.
#[test]
fn allow_builds_keeps_a_trailing_comment() {
    let out =
        run_allow_builds(Some("allowBuilds:\n  esbuild: false # why\n"), &[("esbuild", true)]);
    assert_eq!(out.as_deref(), Some("allowBuilds:\n  esbuild: true # why\n"));
}

/// A quote only delimits a scalar when it opens the value, so an
/// apostrophe inside a plain scalar is a character, not an unterminated
/// quoted string that would swallow a following comment.
#[test]
fn allow_builds_replaces_a_plain_value_containing_a_quote() {
    let out = run_allow_builds(
        Some("allowBuilds:\n  esbuild: don't know yet # decide later\n"),
        &[("esbuild", true)],
    );
    assert_eq!(out.as_deref(), Some("allowBuilds:\n  esbuild: true # decide later\n"));
}

/// A doubled quote is the single-quoted style's escape, so it does not
/// end the scalar either.
#[test]
fn allow_builds_replaces_a_value_with_a_doubled_single_quote() {
    let out = run_allow_builds(
        Some("allowBuilds:\n  esbuild: 'it''s # fine' # real\n"),
        &[("esbuild", true)],
    );
    assert_eq!(out.as_deref(), Some("allowBuilds:\n  esbuild: true # real\n"));
}

#[test]
fn prune_allow_builds_removes_undecided_entry_whose_package_is_not_resolved() {
    let original =
        "allowBuilds:\n  foo: set this to true or false\n  bar: set this to true or false\n";
    let out = run_prune_allow_builds(Some(original), &["foo"]);
    assert_eq!(out.as_deref(), Some("allowBuilds:\n  foo: set this to true or false\n"));
}

#[test]
fn prune_allow_builds_keeps_decided_entries() {
    let original = "allowBuilds:\n  foo: true\n  bar: false\n  baz: set this to true or false\n";
    let out = run_prune_allow_builds(Some(original), &[]);
    assert_eq!(out.as_deref(), Some("allowBuilds:\n  foo: true\n  bar: false\n"));
}

#[test]
fn prune_allow_builds_edits_a_flow_mapping_in_place() {
    let original = "allowBuilds: {foo: true, bar: set this to true or false}\n";
    let out = run_prune_allow_builds(Some(original), &[]);
    assert_eq!(out.as_deref(), Some("allowBuilds: { foo: true }\n"));
}

#[test]
fn prune_allow_builds_keeps_a_flow_mapping_comment_and_quoting() {
    let original = "allowBuilds: {foo: 'set this to true or false', bar: true, baz: 'set this to true or false'} # hey\n";
    let out = run_prune_allow_builds(Some(original), &["foo"]);
    assert_eq!(
        out.as_deref(),
        Some("allowBuilds: { foo: 'set this to true or false', bar: true } # hey\n"),
    );
}

#[test]
fn prune_allow_builds_keeps_keys_with_no_provable_package_name() {
    let original =
        "allowBuilds:\n  foo@git+https://github.com/org/foo.git: set this to true or false\n";
    let out = run_prune_allow_builds(Some(original), &[]);
    assert_eq!(out.as_deref(), Some(original));
}
