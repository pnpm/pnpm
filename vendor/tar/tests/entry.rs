extern crate tar;
extern crate tempfile;

use std::fs::create_dir;
use std::fs::File;
use std::io::Read;

use tempfile::Builder;

#[test]
fn absolute_symlink() {
    let mut ar = tar::Builder::new(Vec::new());

    let mut header = tar::Header::new_gnu();
    header.set_size(0);
    header.set_entry_type(tar::EntryType::Symlink);
    header.set_path("foo").unwrap();
    header.set_link_name("/bar").unwrap();
    header.set_cksum();
    ar.append(&header, &[][..]).unwrap();

    let bytes = ar.into_inner().unwrap();
    let mut ar = tar::Archive::new(&bytes[..]);

    let td = Builder::new().prefix("tar").tempdir().unwrap();
    ar.unpack(td.path()).unwrap();

    td.path().join("foo").symlink_metadata().unwrap();

    let mut ar = tar::Archive::new(&bytes[..]);
    let mut entries = ar.entries().unwrap();
    let entry = entries.next().unwrap().unwrap();
    assert_eq!(&*entry.link_name_bytes().unwrap(), b"/bar");
}

#[test]
fn absolute_hardlink() {
    let td = Builder::new().prefix("tar").tempdir().unwrap();
    let mut ar = tar::Builder::new(Vec::new());

    let mut header = tar::Header::new_gnu();
    header.set_size(0);
    header.set_entry_type(tar::EntryType::Regular);
    header.set_path("foo").unwrap();
    header.set_cksum();
    ar.append(&header, &[][..]).unwrap();

    let mut header = tar::Header::new_gnu();
    header.set_size(0);
    header.set_entry_type(tar::EntryType::Link);
    header.set_path("bar").unwrap();
    // This absolute path under tempdir will be created at unpack time
    header.set_link_name(td.path().join("foo")).unwrap();
    header.set_cksum();
    ar.append(&header, &[][..]).unwrap();

    let bytes = ar.into_inner().unwrap();
    let mut ar = tar::Archive::new(&bytes[..]);

    ar.unpack(td.path()).unwrap();
    td.path().join("foo").metadata().unwrap();
    td.path().join("bar").metadata().unwrap();
}

#[test]
fn relative_hardlink() {
    let mut ar = tar::Builder::new(Vec::new());

    let mut header = tar::Header::new_gnu();
    header.set_size(0);
    header.set_entry_type(tar::EntryType::Regular);
    header.set_path("foo").unwrap();
    header.set_cksum();
    ar.append(&header, &[][..]).unwrap();

    let mut header = tar::Header::new_gnu();
    header.set_size(0);
    header.set_entry_type(tar::EntryType::Link);
    header.set_path("bar").unwrap();
    header.set_link_name("foo").unwrap();
    header.set_cksum();
    ar.append(&header, &[][..]).unwrap();

    let bytes = ar.into_inner().unwrap();
    let mut ar = tar::Archive::new(&bytes[..]);

    let td = Builder::new().prefix("tar").tempdir().unwrap();
    ar.unpack(td.path()).unwrap();
    td.path().join("foo").metadata().unwrap();
    td.path().join("bar").metadata().unwrap();
}

#[test]
fn absolute_link_deref_error() {
    let mut ar = tar::Builder::new(Vec::new());

    let mut header = tar::Header::new_gnu();
    header.set_size(0);
    header.set_entry_type(tar::EntryType::Symlink);
    header.set_path("foo").unwrap();
    header.set_link_name("/").unwrap();
    header.set_cksum();
    ar.append(&header, &[][..]).unwrap();

    let mut header = tar::Header::new_gnu();
    header.set_size(0);
    header.set_entry_type(tar::EntryType::Regular);
    header.set_path("foo/bar").unwrap();
    header.set_cksum();
    ar.append(&header, &[][..]).unwrap();

    let bytes = ar.into_inner().unwrap();
    let mut ar = tar::Archive::new(&bytes[..]);

    let td = Builder::new().prefix("tar").tempdir().unwrap();
    assert!(ar.unpack(td.path()).is_err());
    td.path().join("foo").symlink_metadata().unwrap();
    assert!(File::open(td.path().join("foo").join("bar")).is_err());
}

#[test]
fn relative_link_deref_error() {
    let mut ar = tar::Builder::new(Vec::new());

    let mut header = tar::Header::new_gnu();
    header.set_size(0);
    header.set_entry_type(tar::EntryType::Symlink);
    header.set_path("foo").unwrap();
    header.set_link_name("../../../../").unwrap();
    header.set_cksum();
    ar.append(&header, &[][..]).unwrap();

    let mut header = tar::Header::new_gnu();
    header.set_size(0);
    header.set_entry_type(tar::EntryType::Regular);
    header.set_path("foo/bar").unwrap();
    header.set_cksum();
    ar.append(&header, &[][..]).unwrap();

    let bytes = ar.into_inner().unwrap();
    let mut ar = tar::Archive::new(&bytes[..]);

    let td = Builder::new().prefix("tar").tempdir().unwrap();
    assert!(ar.unpack(td.path()).is_err());
    td.path().join("foo").symlink_metadata().unwrap();
    assert!(File::open(td.path().join("foo").join("bar")).is_err());
}

#[test]
#[cfg(unix)]
fn directory_maintains_permissions() {
    use ::std::os::unix::fs::PermissionsExt;

    let mut ar = tar::Builder::new(Vec::new());

    let mut header = tar::Header::new_gnu();
    header.set_size(0);
    header.set_entry_type(tar::EntryType::Directory);
    header.set_path("foo").unwrap();
    header.set_mode(0o777);
    header.set_cksum();
    ar.append(&header, &[][..]).unwrap();

    let bytes = ar.into_inner().unwrap();
    let mut ar = tar::Archive::new(&bytes[..]);

    let td = Builder::new().prefix("tar").tempdir().unwrap();
    ar.unpack(td.path()).unwrap();
    let f = File::open(td.path().join("foo")).unwrap();
    let md = f.metadata().unwrap();
    assert!(md.is_dir());
    assert_eq!(md.permissions().mode(), 0o40777);
}

#[test]
#[cfg(unix)]
fn set_entry_mask() {
    use ::std::os::unix::fs::PermissionsExt;

    let mut ar = tar::Builder::new(Vec::new());

    let mut header = tar::Header::new_gnu();
    header.set_size(0);
    header.set_entry_type(tar::EntryType::Regular);
    header.set_path("foo").unwrap();
    header.set_mode(0o777);
    header.set_cksum();
    ar.append(&header, &[][..]).unwrap();

    let bytes = ar.into_inner().unwrap();
    let mut ar = tar::Archive::new(&bytes[..]);
    let td = Builder::new().prefix("tar").tempdir().unwrap();
    let foo_path = td.path().join("foo");

    let mut entries = ar.entries().unwrap();
    let mut foo = entries.next().unwrap().unwrap();
    foo.set_mask(0o027);
    foo.unpack(&foo_path).unwrap();

    let f = File::open(foo_path).unwrap();
    let md = f.metadata().unwrap();
    assert!(md.is_file());
    assert_eq!(md.permissions().mode(), 0o100750);
}

#[test]
#[cfg(not(windows))] // dangling symlinks have weird permissions
fn modify_link_just_created() {
    let mut ar = tar::Builder::new(Vec::new());

    let mut header = tar::Header::new_gnu();
    header.set_size(0);
    header.set_entry_type(tar::EntryType::Symlink);
    header.set_path("foo").unwrap();
    header.set_link_name("bar").unwrap();
    header.set_cksum();
    ar.append(&header, &[][..]).unwrap();

    let mut header = tar::Header::new_gnu();
    header.set_size(0);
    header.set_entry_type(tar::EntryType::Regular);
    header.set_path("bar/foo").unwrap();
    header.set_cksum();
    ar.append(&header, &[][..]).unwrap();

    let mut header = tar::Header::new_gnu();
    header.set_size(0);
    header.set_entry_type(tar::EntryType::Regular);
    header.set_path("foo/bar").unwrap();
    header.set_cksum();
    ar.append(&header, &[][..]).unwrap();

    let bytes = ar.into_inner().unwrap();
    let mut ar = tar::Archive::new(&bytes[..]);

    let td = Builder::new().prefix("tar").tempdir().unwrap();
    ar.unpack(td.path()).unwrap();

    File::open(td.path().join("bar/foo")).unwrap();
    File::open(td.path().join("bar/bar")).unwrap();
    File::open(td.path().join("foo/foo")).unwrap();
    File::open(td.path().join("foo/bar")).unwrap();
}

#[test]
#[cfg(not(windows))] // dangling symlinks have weird permissions
fn modify_outside_with_relative_symlink() {
    let mut ar = tar::Builder::new(Vec::new());

    let mut header = tar::Header::new_gnu();
    header.set_size(0);
    header.set_entry_type(tar::EntryType::Symlink);
    header.set_path("symlink").unwrap();
    header.set_link_name("..").unwrap();
    header.set_cksum();
    ar.append(&header, &[][..]).unwrap();

    let mut header = tar::Header::new_gnu();
    header.set_size(0);
    header.set_entry_type(tar::EntryType::Regular);
    header.set_path("symlink/foo/bar").unwrap();
    header.set_cksum();
    ar.append(&header, &[][..]).unwrap();

    let bytes = ar.into_inner().unwrap();
    let mut ar = tar::Archive::new(&bytes[..]);

    let td = Builder::new().prefix("tar").tempdir().unwrap();
    let tar_dir = td.path().join("tar");
    create_dir(&tar_dir).unwrap();
    assert!(ar.unpack(tar_dir).is_err());
    assert!(!td.path().join("foo").exists());
}

#[test]
fn parent_paths_error() {
    let mut ar = tar::Builder::new(Vec::new());

    let mut header = tar::Header::new_gnu();
    header.set_size(0);
    header.set_entry_type(tar::EntryType::Symlink);
    header.set_path("foo").unwrap();
    header.set_link_name("..").unwrap();
    header.set_cksum();
    ar.append(&header, &[][..]).unwrap();

    let mut header = tar::Header::new_gnu();
    header.set_size(0);
    header.set_entry_type(tar::EntryType::Regular);
    header.set_path("foo/bar").unwrap();
    header.set_cksum();
    ar.append(&header, &[][..]).unwrap();

    let bytes = ar.into_inner().unwrap();
    let mut ar = tar::Archive::new(&bytes[..]);

    let td = Builder::new().prefix("tar").tempdir().unwrap();
    assert!(ar.unpack(td.path()).is_err());
    td.path().join("foo").symlink_metadata().unwrap();
    assert!(File::open(td.path().join("foo").join("bar")).is_err());
}

#[test]
#[cfg(unix)]
fn good_parent_paths_ok() {
    use std::path::PathBuf;
    let mut ar = tar::Builder::new(Vec::new());

    let mut header = tar::Header::new_gnu();
    header.set_size(0);
    header.set_entry_type(tar::EntryType::Symlink);
    header.set_path(PathBuf::from("foo").join("bar")).unwrap();
    header
        .set_link_name(PathBuf::from("..").join("bar"))
        .unwrap();
    header.set_cksum();
    ar.append(&header, &[][..]).unwrap();

    let mut header = tar::Header::new_gnu();
    header.set_size(0);
    header.set_entry_type(tar::EntryType::Regular);
    header.set_path("bar").unwrap();
    header.set_cksum();
    ar.append(&header, &[][..]).unwrap();

    let bytes = ar.into_inner().unwrap();
    let mut ar = tar::Archive::new(&bytes[..]);

    let td = Builder::new().prefix("tar").tempdir().unwrap();
    ar.unpack(td.path()).unwrap();
    td.path().join("foo").join("bar").read_link().unwrap();
    let dst = td.path().join("foo").join("bar").canonicalize().unwrap();
    File::open(dst).unwrap();
}

#[test]
fn modify_hard_link_just_created() {
    let mut ar = tar::Builder::new(Vec::new());

    let mut header = tar::Header::new_gnu();
    header.set_size(0);
    header.set_entry_type(tar::EntryType::Link);
    header.set_path("foo").unwrap();
    header.set_link_name("../test").unwrap();
    header.set_cksum();
    ar.append(&header, &[][..]).unwrap();

    let mut header = tar::Header::new_gnu();
    header.set_size(1);
    header.set_entry_type(tar::EntryType::Regular);
    header.set_path("foo").unwrap();
    header.set_cksum();
    ar.append(&header, &b"x"[..]).unwrap();

    let bytes = ar.into_inner().unwrap();
    let mut ar = tar::Archive::new(&bytes[..]);

    let td = Builder::new().prefix("tar").tempdir().unwrap();

    let test = td.path().join("test");
    File::create(&test).unwrap();

    let dir = td.path().join("dir");
    assert!(ar.unpack(&dir).is_err());

    let mut contents = Vec::new();
    File::open(&test)
        .unwrap()
        .read_to_end(&mut contents)
        .unwrap();
    assert_eq!(contents.len(), 0);
}

#[test]
fn modify_symlink_just_created() {
    let mut ar = tar::Builder::new(Vec::new());

    let mut header = tar::Header::new_gnu();
    header.set_size(0);
    header.set_entry_type(tar::EntryType::Symlink);
    header.set_path("foo").unwrap();
    header.set_link_name("../test").unwrap();
    header.set_cksum();
    ar.append(&header, &[][..]).unwrap();

    let mut header = tar::Header::new_gnu();
    header.set_size(1);
    header.set_entry_type(tar::EntryType::Regular);
    header.set_path("foo").unwrap();
    header.set_cksum();
    ar.append(&header, &b"x"[..]).unwrap();

    let bytes = ar.into_inner().unwrap();
    let mut ar = tar::Archive::new(&bytes[..]);

    let td = Builder::new().prefix("tar").tempdir().unwrap();

    let test = td.path().join("test");
    File::create(&test).unwrap();

    let dir = td.path().join("dir");
    ar.unpack(&dir).unwrap();

    let mut contents = Vec::new();
    File::open(&test)
        .unwrap()
        .read_to_end(&mut contents)
        .unwrap();
    assert_eq!(contents.len(), 0);
}

/// Test that unpacking a tarball with a symlink followed by a directory entry
/// with the same name does not allow modifying permissions of arbitrary directories
/// outside the extraction path.
#[test]
#[cfg(unix)]
fn symlink_dir_collision_does_not_modify_external_dir_permissions() {
    use ::std::fs;
    use ::std::os::unix::fs::PermissionsExt;

    let td = Builder::new().prefix("tar").tempdir().unwrap();

    let target_dir = td.path().join("target-dir");
    fs::create_dir(&target_dir).unwrap();
    fs::set_permissions(&target_dir, fs::Permissions::from_mode(0o700)).unwrap();
    let before_mode = fs::metadata(&target_dir).unwrap().permissions().mode() & 0o7777;
    assert_eq!(before_mode, 0o700);

    let extract_dir = td.path().join("extract-dir");
    fs::create_dir(&extract_dir).unwrap();

    let mut ar = tar::Builder::new(Vec::new());

    let mut header = tar::Header::new_gnu();
    header.set_size(0);
    header.set_entry_type(tar::EntryType::Symlink);
    header.set_path("foo").unwrap();
    header.set_link_name(&target_dir).unwrap();
    header.set_mode(0o777);
    header.set_cksum();
    ar.append(&header, &[][..]).unwrap();

    let mut header = tar::Header::new_gnu();
    header.set_size(0);
    header.set_entry_type(tar::EntryType::Directory);
    header.set_path("foo").unwrap();
    header.set_mode(0o777);
    header.set_cksum();
    ar.append(&header, &[][..]).unwrap();

    let bytes = ar.into_inner().unwrap();
    let mut ar = tar::Archive::new(&bytes[..]);

    let result = ar.unpack(&extract_dir);
    assert!(result.is_err());

    let symlink_path = extract_dir.join("foo");
    assert!(symlink_path
        .symlink_metadata()
        .unwrap()
        .file_type()
        .is_symlink());

    let after_mode = fs::metadata(&target_dir).unwrap().permissions().mode() & 0o7777;
    assert_eq!(after_mode, 0o700);
}
