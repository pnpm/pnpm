use super::{Path, ScriptRuntime, generate_sh_shim};
use crate::shim::is_relocatable_shim;

#[test]
fn generate_sh_shim_keeps_paths_outside_the_root_absolute() {
    let root = Path::new("/p");
    let runtime = ScriptRuntime { prog: Some("node".into()), args: String::new() };
    let target = Path::new("/p/node_modules/tool/cli.js");
    let global_shim = Path::new("/g/bin/tool");
    let node_path = ["/p/node_modules/.pnpm/node_modules".to_string()];
    assert_eq!(
        generate_sh_shim(target, global_shim, Some(&runtime), &node_path, Some(root)),
        generate_sh_shim(target, global_shim, Some(&runtime), &node_path, None),
        "a shim outside the root must be written as if there were no root",
    );

    let store_target = Path::new("/store/links/tool/cli.js");
    let shim = Path::new("/p/node_modules/.bin/tool");
    let store_node_path = ["/store/links/node_modules".to_string()];
    let body = generate_sh_shim(store_target, shim, Some(&runtime), &store_node_path, Some(root));
    eprintln!("BODY:\n{body}");
    assert_eq!(
        body.contains("basedir_abs"),
        cfg!(unix),
        "relative exec paths use the physical shim directory",
    );
    assert!(body.contains("  export NODE_PATH=\"/store/links/node_modules\"\n"));
    assert!(body.ends_with("# cmd-shim-target=/store/links/tool/cli.js\n"));
}

#[cfg(unix)]
#[test]
fn relative_node_path_segments_are_sh_escaped() {
    use super::write_executable;
    use std::{fs, process::Command};

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    // The `\$` is the pair that matters: a backslash left as is would consume
    // the escape the `$` gets and let the substitution run.
    let hostile = r#"p$(touch pwned)`touch pwned`"q\$(touch pwned)"#;
    let entry = root.join("node_modules").join(hostile);
    let bin_dir = root.join("node_modules/.bin");
    let target = root.join("node_modules/tool/cli");
    fs::create_dir_all(&entry).unwrap();
    fs::create_dir_all(&bin_dir).unwrap();
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    write_executable(&target, "#!/bin/sh\nprintf '%s' \"$NODE_PATH\"\n");
    let shim = bin_dir.join("tool");
    let node_path = [entry.to_string_lossy().into_owned()];
    write_executable(&shim, &generate_sh_shim(&target, &shim, None, &node_path, Some(root)));

    let output = Command::new(&shim)
        .current_dir(root)
        .env_remove("NODE_PATH")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("STATUS: {:?}\nSTDERR:\n{stderr}", output.status);
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("{}/../{hostile}", fs::canonicalize(&bin_dir).unwrap().display()),
    );
    assert!(!root.join("pwned").exists(), "the directory name ran as a command");
}

#[test]
#[cfg_attr(not(unix), ignore = "relocatable shims are only written on Unix")]
fn in_root_shim_is_written_relative_to_itself_and_only_such_a_shim_is_relocatable() {
    let root = Path::new("/p");
    let shim_dir = Path::new("/p/node_modules/.bin");
    let shim = shim_dir.join("tsc");
    let target = Path::new("/p/node_modules/typescript/bin/tsc");
    let node_path = [
        "/p/node_modules/.pnpm/typescript@5.0.0/node_modules".to_string(),
        "/p/node_modules/.pnpm/node_modules".to_string(),
    ];
    let relocatable = generate_sh_shim(target, &shim, None, &node_path, Some(root));
    eprintln!("BODY:\n{relocatable}");
    assert!(relocatable.contains(concat!(
        "if [ -z \"$NODE_PATH\" ]; then\n",
        "  export NODE_PATH=\"$basedir_abs/../.pnpm/typescript@5.0.0/node_modules:$basedir_abs/../.pnpm/node_modules\"\n",
        "else\n",
        "  export NODE_PATH=\"$basedir_abs/../.pnpm/typescript@5.0.0/node_modules:$basedir_abs/../.pnpm/node_modules:$NODE_PATH\"\n",
        "fi\n",
    )));
    assert!(!relocatable.contains("/p/"), "no path inside the root may stay absolute");
    assert!(relocatable.ends_with("# cmd-shim-target=../typescript/bin/tsc\n"));
    assert!(
        is_relocatable_shim(&relocatable, shim_dir, root),
        "a generated shim, its `$NODE_PATH` passthrough included, must pass",
    );

    let marker = "# cmd-shim-target=../typescript/bin/tsc\n";
    let split_node_path = [
        "/p/node_modules/a\nb/node_modules".to_string(),
        "/old/p/node_modules/.pnpm/node_modules".to_string(),
    ];
    let rejected = [
        (
            "an entry split over two lines",
            generate_sh_shim(target, &shim, None, &split_node_path, Some(root)),
        ),
        (
            "absolute marker",
            relocatable.replace(marker, &format!("# cmd-shim-target={}\n", target.display())),
        ),
        ("no marker", relocatable.replace(marker, "")),
        (
            "marker climbing out",
            relocatable.replace(marker, "# cmd-shim-target=../../../../x/tsc\n"),
        ),
        ("absolute entry", relocatable.replace("$basedir_abs/../.pnpm", "/p/node_modules/.pnpm")),
        (
            "climbing out",
            relocatable.replace("$basedir_abs/../.pnpm", "$basedir_abs/../../../../x"),
        ),
    ];
    for (label, shim_content) in rejected {
        assert!(!is_relocatable_shim(&shim_content, shim_dir, root), "{label}:\n{shim_content}");
    }
}
