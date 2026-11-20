use super::CrossMountFs;
use crate::cli_args::doctor::{
    CheckStatus,
    filesystem::check_filesystem_capabilities,
    probe_path::windows_acl::{
        WindowsAccessRule,
        WindowsDirSecurity,
        WindowsPathRole,
        can_write_windows_dir_security,
        is_safe_windows_dir_security,
        is_trusted_windows_principal,
        is_unc_windows_path,
    },
};
use pnpm_config::Config;
use pnpm_fs::Host;
use pnpm_store_dir::StoreDir;
use pretty_assertions::assert_eq;
use rustc_hash::FxHashSet;
#[cfg(any(unix, windows))]
use std::fs;
use std::path::Path;

#[cfg(unix)]
#[test]
fn filesystem_check_warns_without_probing_beneath_world_or_group_writable_ancestor() {
    use std::os::unix::fs::PermissionsExt as _;

    let sandbox = tempfile::tempdir().expect("create sandbox");
    for mode in [0o777, 0o775] {
        let unsafe_parent = sandbox
            .path()
            .join(format!("unsafe-{mode:o}"));
        let project_dir = unsafe_parent.join("proj");
        fs::create_dir_all(&project_dir).expect("create project dir");
        fs::set_permissions(&unsafe_parent, fs::Permissions::from_mode(mode))
            .expect("chmod unsafe ancestor");

        let modules_dir = project_dir.join("node_modules");
        let mut config = Config {
            store_dir: StoreDir::new(sandbox.path().join("store")),
            modules_dir: modules_dir.clone(),
            install_state_dir: modules_dir.join(".pnpm"),
            ..Config::default()
        };
        config.apply_global_virtual_store_derivation(false, false);

        let result = check_filesystem_capabilities::<Host>(&config);
        fs::set_permissions(&unsafe_parent, fs::Permissions::from_mode(0o755))
            .expect("restore permissions");

        dbg!(mode, &result);
        assert_eq!(result.status, CheckStatus::Warn);
        assert_eq!(
            result.detail.as_deref(),
            Some("cannot safely probe link support in a directory writable by other users"),
        );
        assert_eq!(fs::read_dir(&project_dir).expect("read project dir").count(), 0);
    }
}

#[cfg(windows)]
#[test]
fn filesystem_check_warns_without_probing_beneath_shared_windows_ancestor() {
    use std::process::Command;

    let sandbox = tempfile::tempdir().expect("create sandbox");
    let shared_parent = sandbox.path().join("shared-everyone");
    let project_dir = shared_parent.join("proj");
    fs::create_dir_all(&project_dir).expect("create project dir");

    let grant = Command::new("icacls")
        .arg(&shared_parent)
        .args(["/grant", "*S-1-1-0:(OI)(CI)(F)"])
        .output()
        .expect("run icacls grant");
    assert!(grant.status.success(), "icacls grant failed: {grant:?}");

    let modules_dir = project_dir.join("node_modules");
    let mut config = Config {
        store_dir: StoreDir::new(sandbox.path().join("store")),
        modules_dir: modules_dir.clone(),
        install_state_dir: modules_dir.join(".pnpm"),
        ..Config::default()
    };
    config.apply_global_virtual_store_derivation(false, false);

    let result = check_filesystem_capabilities::<Host>(&config);
    let _ = Command::new("icacls")
        .arg(&shared_parent)
        .args(["/reset", "/T"])
        .output();

    dbg!(&result);
    assert_eq!(result.status, CheckStatus::Warn);
    assert_eq!(
        result.detail.as_deref(),
        Some("cannot safely probe link support in a directory writable by other users"),
    );
    assert_eq!(fs::read_dir(&project_dir).expect("read project dir").count(), 0);
}

#[test]
fn windows_dir_security_accepts_standard_system_roots() {
    let current_user = "S-1-5-21-10-20-30-1001";
    let admins: FxHashSet<String> = std::iter::once("S-1-5-21-10-20-30-1000".to_owned()).collect();
    let root_security = WindowsDirSecurity {
        ok: true,
        null_acl: false,
        owner: Some("S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464".to_owned()),
        rules: vec![
            WindowsAccessRule {
                sid: "S-1-5-18".to_owned(),
                allow: true,
                mask: 0x001f_01ff,
                prop: 0,
            },
            WindowsAccessRule {
                sid: "S-1-5-32-544".to_owned(),
                allow: true,
                mask: 0x001f_01ff,
                prop: 0,
            },
            WindowsAccessRule {
                sid: "S-1-3-0".to_owned(),
                allow: true,
                mask: 0x1000_0000,
                prop: 2,
            },
            WindowsAccessRule {
                sid: "S-1-5-32-545".to_owned(),
                allow: true,
                mask: 0x0012_00a9,
                prop: 0,
            },
            WindowsAccessRule {
                sid: "S-1-5-32-545".to_owned(),
                allow: true,
                mask: -1_610_612_736,
                prop: 2,
            },
            WindowsAccessRule {
                sid: "S-1-5-32-545".to_owned(),
                allow: true,
                mask: 0x0000_0004,
                prop: 0,
            },
            WindowsAccessRule {
                sid: "S-1-5-32-545".to_owned(),
                allow: true,
                mask: 0x0000_0002,
                prop: 2,
            },
        ],
    };
    assert!(is_safe_windows_dir_security(
        &root_security,
        current_user,
        &admins,
        WindowsPathRole::ProbeParent,
    ));
}

#[test]
fn windows_dir_security_accepts_client_root_inherit_only_modify_for_strict_ancestors_only() {
    let current_user = "S-1-5-21-10-20-30-1001";
    let admins: FxHashSet<String> = FxHashSet::default();
    let client_root = WindowsDirSecurity {
        ok: true,
        null_acl: false,
        owner: Some("S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464".to_owned()),
        rules: vec![
            WindowsAccessRule {
                sid: "S-1-5-18".to_owned(),
                allow: true,
                mask: 0x001f_01ff,
                prop: 0,
            },
            WindowsAccessRule {
                sid: "S-1-5-32-544".to_owned(),
                allow: true,
                mask: 0x001f_01ff,
                prop: 0,
            },
            WindowsAccessRule {
                sid: "S-1-5-32-545".to_owned(),
                allow: true,
                mask: 0x0012_00a9,
                prop: 0,
            },
            WindowsAccessRule {
                sid: "S-1-5-11".to_owned(),
                allow: true,
                mask: 0x0013_01bf,
                prop: 2,
            },
            WindowsAccessRule {
                sid: "S-1-5-11".to_owned(),
                allow: true,
                mask: 0x0000_0004,
                prop: 0,
            },
        ],
    };
    assert!(is_safe_windows_dir_security(
        &client_root,
        current_user,
        &admins,
        WindowsPathRole::StrictAncestor,
    ));
    assert!(!is_safe_windows_dir_security(
        &client_root,
        current_user,
        &admins,
        WindowsPathRole::ProbeParent,
    ));
}

#[test]
fn windows_dir_security_rejects_shared_untrusted_owners_and_null_acls() {
    let current_user = "S-1-5-21-10-20-30-1001";
    let admins: FxHashSet<String> = FxHashSet::default();

    let shared_modify = WindowsDirSecurity {
        ok: true,
        null_acl: false,
        owner: Some(current_user.to_owned()),
        rules: vec![WindowsAccessRule {
            sid: "S-1-5-32-545".to_owned(),
            allow: true,
            mask: 0x0013_01bf,
            prop: 0,
        }],
    };
    assert!(!is_safe_windows_dir_security(
        &shared_modify,
        current_user,
        &admins,
        WindowsPathRole::ProbeParent,
    ));

    let untrusted_owner = WindowsDirSecurity {
        ok: true,
        null_acl: false,
        owner: Some("S-1-5-21-10-20-30-1002".to_owned()),
        rules: vec![WindowsAccessRule {
            sid: "S-1-5-18".to_owned(),
            allow: true,
            mask: 0x001f_01ff,
            prop: 0,
        }],
    };
    assert!(!is_safe_windows_dir_security(
        &untrusted_owner,
        current_user,
        &admins,
        WindowsPathRole::ProbeParent,
    ));

    let null_acl = WindowsDirSecurity {
        ok: true,
        null_acl: true,
        owner: Some(current_user.to_owned()),
        rules: vec![WindowsAccessRule {
            sid: "S-1-1-0".to_owned(),
            allow: true,
            mask: 0x001f_01ff,
            prop: 0,
        }],
    };
    assert!(!is_safe_windows_dir_security(
        &null_acl,
        current_user,
        &admins,
        WindowsPathRole::ProbeParent,
    ));

    let denied_write = WindowsDirSecurity {
        ok: true,
        null_acl: false,
        owner: Some(current_user.to_owned()),
        rules: vec![
            WindowsAccessRule { sid: "S-1-1-0".to_owned(), allow: true, mask: 0x6, prop: 0 },
            WindowsAccessRule { sid: current_user.to_owned(), allow: false, mask: 0x2, prop: 0 },
        ],
    };
    let members: FxHashSet<String> =
        [current_user.to_owned(), "S-1-1-0".to_owned()].into_iter().collect();
    assert!(!can_write_windows_dir_security(&denied_write, &members));
}

#[test]
fn windows_dir_security_rejects_untrusted_service_sids_and_reparse_rights() {
    let current_user = "S-1-5-21-10-20-30-1001";
    let admins: FxHashSet<String> = FxHashSet::default();
    let other_service_sid = "S-1-5-80-111-222-333-444-555";
    assert!(!is_trusted_windows_principal(other_service_sid, current_user, &admins));

    let service_owned = WindowsDirSecurity {
        ok: true,
        null_acl: false,
        owner: Some(other_service_sid.to_owned()),
        rules: vec![WindowsAccessRule {
            sid: "S-1-5-18".to_owned(),
            allow: true,
            mask: 0x001f_01ff,
            prop: 0,
        }],
    };
    assert!(!is_safe_windows_dir_security(
        &service_owned,
        current_user,
        &admins,
        WindowsPathRole::ProbeParent,
    ));

    for mask in [0x0000_0100, 0x4000_0000] {
        let reparse_ace = WindowsDirSecurity {
            ok: true,
            null_acl: false,
            owner: Some(current_user.to_owned()),
            rules: vec![WindowsAccessRule {
                sid: "S-1-5-32-545".to_owned(),
                allow: true,
                mask,
                prop: 0,
            }],
        };
        assert!(!is_safe_windows_dir_security(
            &reparse_ace,
            current_user,
            &admins,
            WindowsPathRole::ProbeParent,
        ));
    }

    assert!(is_unc_windows_path(Path::new(r"\\server\share\cache")));
    assert!(!is_unc_windows_path(Path::new(r"C:\Users\demo\cache")));
}

#[cfg(any(unix, windows))]
#[test]
fn filesystem_check_resolves_symlinks_before_parent_segments() {
    let sandbox = tempfile::tempdir().expect("create sandbox");
    let mount_a_sub = sandbox.path().join("mount-a/sub");
    let mount_b = sandbox.path().join("mount-b");
    let project_dir = mount_b.join("proj");
    fs::create_dir_all(&mount_a_sub).expect("create mount-a/sub");
    fs::create_dir_all(&project_dir).expect("create mount-b/proj");

    let link_path = mount_b.join("link");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&mount_a_sub, &link_path).expect("create symlink");
    #[cfg(windows)]
    if let Err(error) = std::os::windows::fs::symlink_dir(&mount_a_sub, &link_path) {
        if error.raw_os_error() == Some(1314) {
            eprintln!("skipping symlink assertion without Windows symlink privilege: {error}");
            return;
        }
        panic!("create symlink: {error}");
    }

    let modules_dir = project_dir.join("node_modules");
    let mut config = Config {
        store_dir: StoreDir::new(link_path.join("../store")),
        modules_dir: modules_dir.clone(),
        install_state_dir: modules_dir.join(".pnpm"),
        ..Config::default()
    };
    config.apply_global_virtual_store_derivation(false, false);

    let result = check_filesystem_capabilities::<CrossMountFs>(&config);
    dbg!(&result);
    assert_eq!(result.status, CheckStatus::Warn);
}
