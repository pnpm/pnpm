use super::{
    LOCKFILE_NAME,
    LockedPackage,
    LockedPath,
    LockedSystem,
    Lockfile,
    LockfileError,
};
use pnpz_nix_store::{
    NarHash,
    StorePath,
};
use pretty_assertions::assert_eq;
use std::fs;
use tempfile::TempDir;

const HELLO: &str = "/nix/store/1q8w6gl1ll0mwfkqc3c2yx005s6wwfrl-hello-2.12.1";
const GLIBC: &str = "/nix/store/h4wfwic161kxrr74jlzla5lsm28hgary-glibc-2.44-25";

fn lockfile() -> Lockfile {
    let path = |text: &str| StorePath::from_absolute(text).unwrap();
    let locked = |references: &[&str]| LockedPath {
        nar_hash: NarHash::parse("sha256-47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=").unwrap(),
        nar_size: 120,
        references: references
            .iter()
            .map(|text| StorePath::from_absolute(text).unwrap())
            .collect(),
    };
    let system = LockedSystem {
        nixpkgs: "55ba7f49ef2962b42cbd126522b7df5f95037679".to_string(),
        attr: "hello".to_string(),
        outputs: [("out".to_string(), path(HELLO))].into(),
    };
    let package = LockedPackage {
        specifier: "latest".to_string(),
        version: "2.12.1".to_string(),
        systems: [("x86_64-linux".to_string(), system)].into(),
    };
    let closure = [(path(HELLO), locked(&[GLIBC, HELLO])), (path(GLIBC), locked(&[]))].into();
    Lockfile {
        lockfile_version: 1,
        packages: [("hello".to_string(), package)].into(),
        closures: [("x86_64-linux".to_string(), closure)].into(),
    }
}

#[test]
fn writes_the_documented_format() {
    let temp = TempDir::new().unwrap();
    lockfile().save(temp.path()).unwrap();
    let text = fs::read_to_string(temp.path().join(LOCKFILE_NAME)).unwrap();
    assert_eq!(
        text,
        "\
lockfileVersion: 1
packages:
  hello:
    specifier: latest
    version: 2.12.1
    systems:
      x86_64-linux:
        nixpkgs: 55ba7f49ef2962b42cbd126522b7df5f95037679
        attr: hello
        outputs:
          out: /nix/store/1q8w6gl1ll0mwfkqc3c2yx005s6wwfrl-hello-2.12.1
closures:
  x86_64-linux:
    /nix/store/1q8w6gl1ll0mwfkqc3c2yx005s6wwfrl-hello-2.12.1:
      narHash: sha256-47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=
      narSize: 120
      references:
      - /nix/store/h4wfwic161kxrr74jlzla5lsm28hgary-glibc-2.44-25
      - /nix/store/1q8w6gl1ll0mwfkqc3c2yx005s6wwfrl-hello-2.12.1
    /nix/store/h4wfwic161kxrr74jlzla5lsm28hgary-glibc-2.44-25:
      narHash: sha256-47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=
      narSize: 120
",
    );
    assert_eq!(Lockfile::load(temp.path()).unwrap(), Some(lockfile()));
}

#[test]
fn a_missing_lockfile_is_none() {
    let temp = TempDir::new().unwrap();
    assert_eq!(Lockfile::load(temp.path()).unwrap(), None);
}

#[test]
fn rejects_a_store_path_outside_the_store() {
    let temp = TempDir::new().unwrap();
    lockfile().save(temp.path()).unwrap();
    let path = temp.path().join(LOCKFILE_NAME);
    let text = fs::read_to_string(&path).unwrap().replace("out: /nix/store/", "out: /usr/");
    fs::write(&path, text).unwrap();
    let error = Lockfile::load(temp.path()).unwrap_err();
    assert!(matches!(error, LockfileError::Invalid { .. }), "{error}");
}

#[test]
fn rejects_another_lockfile_version() {
    let temp = TempDir::new().unwrap();
    let mut newer = lockfile();
    newer.lockfile_version = 2;
    newer.save(temp.path()).unwrap();
    let error = Lockfile::load(temp.path()).unwrap_err();
    assert!(matches!(error, LockfileError::UnsupportedVersion { found: 2, .. }), "{error}");
}

#[test]
fn lists_the_environment_of_a_system() {
    let lockfile = lockfile();
    let paths: Vec<String> = lockfile
        .environment_paths("x86_64-linux")
        .into_iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(paths, [HELLO]);
    assert_eq!(lockfile.environment_paths("aarch64-darwin"), Vec::<&StorePath>::new());
}
