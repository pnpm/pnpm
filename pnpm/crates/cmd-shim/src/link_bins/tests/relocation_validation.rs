use crate::{link_bins::bin_dir_is_relocatable, shim::is_relocatable_shim};
use std::{fs, os::unix::fs::symlink};
use tempfile::tempdir;

#[test]
fn relative_bin_links_must_resolve_inside_the_physical_root() {
    let tmp = tempdir().unwrap();
    let root = tmp.path().join("project");
    let bins = root.join(".bin");
    fs::create_dir_all(&bins).unwrap();
    fs::create_dir_all(tmp.path().join("outside")).unwrap();
    fs::create_dir_all(root.join("internal")).unwrap();
    let alias = root.join("alias");
    symlink("internal", &alias).unwrap();
    symlink("../alias/missing/cli", bins.join("tool")).unwrap();
    assert!(bin_dir_is_relocatable(&bins, &root), "missing internal tail is valid");
    let root_alias = tmp.path().join("project-alias");
    symlink(&root, &root_alias).unwrap();
    assert!(bin_dir_is_relocatable(&root_alias.join(".bin"), &root_alias));

    fs::remove_file(&alias).unwrap();
    symlink("../outside", &alias).unwrap();
    assert!(!bin_dir_is_relocatable(&bins, &root), "ancestor escapes physically");
    fs::remove_file(&alias).unwrap();
    symlink("alias", &alias).unwrap();
    assert!(!bin_dir_is_relocatable(&bins, &root), "unresolvable loop fails closed");
}

#[test]
fn shim_markers_and_node_paths_must_resolve_inside_the_physical_root() {
    let tmp = tempdir().unwrap();
    let root = tmp.path().join("project");
    let bins = root.join(".bin");
    fs::create_dir_all(&bins).unwrap();
    fs::create_dir_all(root.join("internal")).unwrap();
    fs::create_dir_all(tmp.path().join("outside")).unwrap();
    let alias = root.join("alias");
    symlink("internal", &alias).unwrap();
    let marker = "# cmd-shim-target=../alias/missing/cli\n";
    let node_path = concat!(
        "# cmd-shim-target=../internal/cli\n",
        "export NODE_PATH=\"$basedir_abs/../alias/missing/modules:$NODE_PATH\"\n",
    );
    for content in [marker, node_path] {
        assert!(is_relocatable_shim(content, &bins, &root));
    }
    fs::remove_file(&alias).unwrap();
    symlink("../outside", &alias).unwrap();
    for content in [marker, node_path] {
        assert!(!is_relocatable_shim(content, &bins, &root), "{content}");
    }
}

#[test]
fn escaped_node_path_alias_is_checked_using_its_literal_name() {
    let tmp = tempdir().unwrap();
    let root = tmp.path().join("project");
    let bins = root.join(".bin");
    fs::create_dir_all(&bins).unwrap();
    fs::create_dir_all(root.join("internal")).unwrap();
    fs::create_dir_all(tmp.path().join("outside")).unwrap();
    let alias = root.join(r#"alias$`"\"#);
    symlink("internal", &alias).unwrap();
    let content = concat!(
        "# cmd-shim-target=../internal/cli\n",
        r#"export NODE_PATH="$basedir_abs/../alias\$\`\"\\/missing/modules""#,
        "\n",
    );
    assert!(is_relocatable_shim(content, &bins, &root));
    fs::remove_file(&alias).unwrap();
    symlink("../outside", &alias).unwrap();
    assert!(!is_relocatable_shim(content, &bins, &root));
}

#[test]
fn node_path_validation_rejects_malformed_escapes_and_shell_expansion() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    let bins = root.join(".bin");
    fs::create_dir_all(&bins).unwrap();
    let entries = [r"../invalid\q", r"../trailing\", "../$HOME", "../`pwd`", r#"../unquoted"name"#];
    for entry in entries {
        let content =
            format!("# cmd-shim-target=../cli\nexport NODE_PATH=\"$basedir_abs/{entry}\"\n");
        assert!(!is_relocatable_shim(&content, &bins, root), "{content}");
    }
}

#[test]
fn validators_reject_external_bin_directories_and_unresolvable_roots() {
    let tmp = tempdir().unwrap();
    let root = tmp.path().join("project");
    let bins = root.join(".bin");
    let outside = tmp.path().join("outside");
    fs::create_dir_all(&bins).unwrap();
    fs::create_dir_all(&outside).unwrap();
    let content = "# cmd-shim-target=cli\n";
    assert!(bin_dir_is_relocatable(&bins, &root));
    assert!(is_relocatable_shim(content, &bins, &root));

    let alias = root.join("bin-alias");
    symlink("../outside", &alias).unwrap();
    assert!(!bin_dir_is_relocatable(&alias, &root));
    assert!(!is_relocatable_shim(content, &alias, &root));

    let unresolved = tmp.path().join("loop");
    symlink("loop", &unresolved).unwrap();
    for (bin_dir, relocation_root) in [(&unresolved, &root), (&bins, &unresolved)] {
        assert!(!bin_dir_is_relocatable(bin_dir, relocation_root));
        assert!(!is_relocatable_shim(content, bin_dir, relocation_root));
    }
}

#[test]
fn a_bin_file_past_the_size_bound_is_refused() {
    let tmp = tempdir().unwrap();
    let root = tmp.path();
    let bins = root.join(".bin");
    fs::create_dir_all(&bins).unwrap();
    let shim = bins.join("tool");
    let marker = "# cmd-shim-target=../cli\n";
    fs::write(&shim, marker).unwrap();
    assert!(bin_dir_is_relocatable(&bins, root));

    let padding = " ".repeat(64 * 1024);
    fs::write(&shim, format!("{marker}{padding}")).unwrap();
    assert!(!bin_dir_is_relocatable(&bins, root), "a shim past the size bound fails closed");
}
