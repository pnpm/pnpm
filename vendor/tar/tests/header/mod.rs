use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;
use std::{mem, thread, time};

use tempfile::Builder;

use tar::{GnuHeader, Header, HeaderMode};

#[test]
fn default_gnu() {
    let mut h = Header::new_gnu();
    assert!(h.as_gnu().is_some());
    assert!(h.as_gnu_mut().is_some());
    assert!(h.as_ustar().is_none());
    assert!(h.as_ustar_mut().is_none());
}

#[test]
fn goto_old() {
    let mut h = Header::new_old();
    assert!(h.as_gnu().is_none());
    assert!(h.as_gnu_mut().is_none());
    assert!(h.as_ustar().is_none());
    assert!(h.as_ustar_mut().is_none());
}

#[test]
fn goto_ustar() {
    let mut h = Header::new_ustar();
    assert!(h.as_gnu().is_none());
    assert!(h.as_gnu_mut().is_none());
    assert!(h.as_ustar().is_some());
    assert!(h.as_ustar_mut().is_some());
}

#[test]
fn link_name() {
    let mut h = Header::new_gnu();
    h.set_link_name("foo").unwrap();
    assert_eq!(h.link_name().unwrap().unwrap().to_str(), Some("foo"));
    h.set_link_name("../foo").unwrap();
    assert_eq!(h.link_name().unwrap().unwrap().to_str(), Some("../foo"));
    h.set_link_name("foo/bar").unwrap();
    assert_eq!(h.link_name().unwrap().unwrap().to_str(), Some("foo/bar"));
    h.set_link_name("foo\\ba").unwrap();
    if cfg!(windows) {
        assert_eq!(h.link_name().unwrap().unwrap().to_str(), Some("foo/ba"));
    } else {
        assert_eq!(h.link_name().unwrap().unwrap().to_str(), Some("foo\\ba"));
    }

    let name = "foo\\bar\0";
    for (slot, val) in h.as_old_mut().linkname.iter_mut().zip(name.as_bytes()) {
        *slot = *val;
    }
    assert_eq!(h.link_name().unwrap().unwrap().to_str(), Some("foo\\bar"));

    assert!(h.set_link_name("\0").is_err());

    h.set_link_name("/foo").unwrap();
    assert_eq!(h.link_name().unwrap().unwrap().to_str(), Some("/foo"));
}

#[test]
fn mtime() {
    let h = Header::new_gnu();
    assert_eq!(h.mtime().unwrap(), 0);

    let h = Header::new_ustar();
    assert_eq!(h.mtime().unwrap(), 0);

    let h = Header::new_old();
    assert_eq!(h.mtime().unwrap(), 0);
}

#[test]
fn user_and_group_name() {
    let mut h = Header::new_gnu();
    h.set_username("foo").unwrap();
    h.set_groupname("bar").unwrap();
    assert_eq!(h.username().unwrap(), Some("foo"));
    assert_eq!(h.groupname().unwrap(), Some("bar"));

    h = Header::new_ustar();
    h.set_username("foo").unwrap();
    h.set_groupname("bar").unwrap();
    assert_eq!(h.username().unwrap(), Some("foo"));
    assert_eq!(h.groupname().unwrap(), Some("bar"));

    h = Header::new_old();
    assert_eq!(h.username().unwrap(), None);
    assert_eq!(h.groupname().unwrap(), None);
    assert!(h.set_username("foo").is_err());
    assert!(h.set_groupname("foo").is_err());
}

#[test]
fn dev_major_minor() {
    let mut h = Header::new_gnu();
    h.set_device_major(1).unwrap();
    h.set_device_minor(2).unwrap();
    assert_eq!(h.device_major().unwrap(), Some(1));
    assert_eq!(h.device_minor().unwrap(), Some(2));

    h = Header::new_ustar();
    h.set_device_major(1).unwrap();
    h.set_device_minor(2).unwrap();
    assert_eq!(h.device_major().unwrap(), Some(1));
    assert_eq!(h.device_minor().unwrap(), Some(2));

    h.as_ustar_mut().unwrap().dev_minor[0] = 0x7f;
    h.as_ustar_mut().unwrap().dev_major[0] = 0x7f;
    assert!(h.device_major().is_err());
    assert!(h.device_minor().is_err());

    h.as_ustar_mut().unwrap().dev_minor[0] = b'g';
    h.as_ustar_mut().unwrap().dev_major[0] = b'h';
    assert!(h.device_major().is_err());
    assert!(h.device_minor().is_err());

    h = Header::new_old();
    assert_eq!(h.device_major().unwrap(), None);
    assert_eq!(h.device_minor().unwrap(), None);
    assert!(h.set_device_major(1).is_err());
    assert!(h.set_device_minor(1).is_err());
}

#[test]
fn set_path() {
    let mut h = Header::new_gnu();
    h.set_path("foo").unwrap();
    assert_eq!(h.path().unwrap().to_str(), Some("foo"));
    h.set_path("foo/").unwrap();
    assert_eq!(h.path().unwrap().to_str(), Some("foo/"));
    h.set_path("foo/bar").unwrap();
    assert_eq!(h.path().unwrap().to_str(), Some("foo/bar"));
    h.set_path("foo\\bar").unwrap();
    if cfg!(windows) {
        assert_eq!(h.path().unwrap().to_str(), Some("foo/bar"));
    } else {
        assert_eq!(h.path().unwrap().to_str(), Some("foo\\bar"));
    }

    // set_path documentation explicitly states it removes any ".", signifying the
    // current directory, from the path. This test ensures that documented
    // behavior occurs
    h.set_path("./control").unwrap();
    assert_eq!(h.path().unwrap().to_str(), Some("control"));

    let long_name = "foo".repeat(100);
    let medium1 = "foo".repeat(52);
    let medium2 = "fo/".repeat(52);

    assert!(h.set_path(&long_name).is_err());
    assert!(h.set_path(&medium1).is_err());
    assert!(h.set_path(&medium2).is_err());
    assert!(h.set_path("\0").is_err());

    assert!(h.set_path("..").is_err());
    assert!(h.set_path("foo/..").is_err());
    assert!(h.set_path("foo/../bar").is_err());
    assert!(h.set_path("/foo").is_err());

    assert!(h.set_path_absolute("/foo").is_ok());
    assert_eq!(h.path().unwrap().to_str(), Some("/foo"));

    h = Header::new_ustar();
    h.set_path("foo").unwrap();
    assert_eq!(h.path().unwrap().to_str(), Some("foo"));

    assert!(h.set_path(&long_name).is_err());
    assert!(h.set_path(&medium1).is_err());
    h.set_path(&medium2).unwrap();
    assert_eq!(h.path().unwrap().to_str(), Some(&medium2[..]));
}

#[test]
fn set_ustar_path_hard() {
    let mut h = Header::new_ustar();
    let p = Path::new("a").join(vec!["a"; 100].join(""));
    h.set_path(&p).unwrap();
    assert_eq!(h.path().unwrap(), p);
}

#[test]
fn set_metadata_deterministic() {
    let td = Builder::new().prefix("tar-rs").tempdir().unwrap();
    let tmppath = td.path().join("tmpfile");

    fn mk_header(path: &Path, readonly: bool) -> Result<Header, io::Error> {
        let mut file = File::create(path).unwrap();
        file.write_all(b"c").unwrap();
        let mut perms = file.metadata().unwrap().permissions();
        perms.set_readonly(readonly);
        fs::set_permissions(path, perms).unwrap();
        let mut h = Header::new_ustar();
        h.set_metadata_in_mode(&path.metadata().unwrap(), HeaderMode::Deterministic);
        Ok(h)
    }

    // Create "the same" File twice in a row, one second apart, with differing readonly values.
    let one = mk_header(tmppath.as_path(), false).unwrap();
    thread::sleep(time::Duration::from_millis(1050));
    let two = mk_header(tmppath.as_path(), true).unwrap();

    // Always expected to match.
    assert_eq!(one.size().unwrap(), two.size().unwrap());
    assert_eq!(one.path().unwrap(), two.path().unwrap());
    assert_eq!(one.mode().unwrap(), two.mode().unwrap());

    // Would not match without `Deterministic`.
    assert_eq!(one.mtime().unwrap(), two.mtime().unwrap());
    assert_eq!(one.mtime().unwrap(), 1153704088);
    // TODO: No great way to validate that these would not be filled, but
    // check them anyway.
    assert_eq!(one.uid().unwrap(), two.uid().unwrap());
    assert_eq!(one.gid().unwrap(), two.gid().unwrap());
}

#[test]
fn extended_numeric_format() {
    let mut h: GnuHeader = unsafe { mem::zeroed() };
    h.as_header_mut().set_size(42);
    assert_eq!(h.size, [48, 48, 48, 48, 48, 48, 48, 48, 48, 53, 50, 0]);
    h.as_header_mut().set_size(8589934593);
    assert_eq!(h.size, [0x80, 0, 0, 0, 0, 0, 0, 0x02, 0, 0, 0, 1]);
    h.as_header_mut().set_size(44);
    assert_eq!(h.size, [48, 48, 48, 48, 48, 48, 48, 48, 48, 53, 52, 0]);
    h.size = [0x80, 0, 0, 0, 0, 0, 0, 0x02, 0, 0, 0, 0];
    assert_eq!(h.as_header().entry_size().unwrap(), 0x0200000000);
    h.size = [48, 48, 48, 48, 48, 48, 48, 48, 48, 53, 51, 0];
    assert_eq!(h.as_header().entry_size().unwrap(), 43);

    h.as_header_mut().set_gid(42);
    assert_eq!(h.gid, [48, 48, 48, 48, 48, 53, 50, 0]);
    assert_eq!(h.as_header().gid().unwrap(), 42);
    h.as_header_mut().set_gid(0x7fffffffffffffff);
    assert_eq!(h.gid, [0xff; 8]);
    assert_eq!(h.as_header().gid().unwrap(), 0x7fffffffffffffff);
    h.uid = [0x80, 0x00, 0x00, 0x00, 0x12, 0x34, 0x56, 0x78];
    assert_eq!(h.as_header().uid().unwrap(), 0x12345678);

    h.mtime = [
        0x80, 0, 0, 0, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef,
    ];
    assert_eq!(h.as_header().mtime().unwrap(), 0x0123456789abcdef);

    h.realsize = [0x80, 0, 0, 0, 0, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde];
    assert_eq!(h.real_size().unwrap(), 0x00123456789abcde);
    h.sparse[0].offset = [0x80, 0, 0, 0, 0, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd];
    assert_eq!(h.sparse[0].offset().unwrap(), 0x000123456789abcd);
    h.sparse[0].numbytes = [0x80, 0, 0, 0, 0, 0, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc];
    assert_eq!(h.sparse[0].length().unwrap(), 0x0000123456789abc);
}

#[test]
fn byte_slice_conversion() {
    let h = Header::new_gnu();
    let b: &[u8] = h.as_bytes();
    let b_conv: &[u8] = Header::from_byte_slice(h.as_bytes()).as_bytes();
    assert_eq!(b, b_conv);
}
