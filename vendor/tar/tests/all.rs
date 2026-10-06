extern crate filetime;
extern crate tar;
extern crate tempfile;
#[cfg(all(unix, feature = "xattr"))]
extern crate xattr;

use std::fs::{self, File};
use std::io::prelude::*;
use std::io::{self, BufWriter, Cursor};
use std::iter::repeat;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use filetime::FileTime;
use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};
use tar::{Archive, Builder, Entries, Entry, EntryType, Header, HeaderMode};
use tempfile::{Builder as TempBuilder, TempDir};

/// A reader wrapper that returns partial results from `read()` to exercise
/// parsers that might assume `read()` fills the entire buffer.
///
/// Each call returns between 1 and buf.len() bytes, biased toward small
/// reads by taking the minimum of two uniform samples. This gives roughly
/// quadratic density toward 1, so small reads (1-10 bytes) occur frequently
/// while large reads still happen. Uses a deterministic seeded RNG so
/// tests remain reproducible.
struct RandomReader<R> {
    inner: R,
    rng: SmallRng,
}

impl<R> RandomReader<R> {
    fn new(inner: R) -> Self {
        RandomReader {
            inner,
            rng: SmallRng::seed_from_u64(0),
        }
    }
}

impl<R: Read> Read for RandomReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return self.inner.read(buf);
        }
        // Take the min of two uniform samples to bias toward small reads.
        let a = self.rng.gen_range(1..=buf.len());
        let b = self.rng.gen_range(1..=buf.len());
        self.inner.read(&mut buf[..a.min(b)])
    }
}

/// Convenience: wrap a byte slice in a RandomReader<Cursor<_>>.
///
/// The RNG is seeded from a hash of the data, so different archives
/// exercise different read-size sequences while remaining deterministic.
fn random_cursor_reader<D: AsRef<[u8]>>(data: D) -> RandomReader<Cursor<D>> {
    RandomReader::new(Cursor::new(data))
}

macro_rules! tar {
    ($e:expr) => {
        &include_bytes!(concat!("archives/", $e))[..]
    };
}

mod header;

/// test that we can concatenate the simple.tar archive and extract the same entries twice when we
/// use the ignore_zeros option.
#[test]
fn simple_concat() {
    let bytes = tar!("simple.tar");
    let mut archive_bytes = Vec::new();
    archive_bytes.extend(bytes);

    let original_names: Vec<String> =
        decode_names(&mut Archive::new(random_cursor_reader(&archive_bytes)));
    let expected: Vec<&str> = original_names.iter().map(|n| n.as_str()).collect();

    // concat two archives (with null in-between);
    archive_bytes.extend(bytes);

    // test now that when we read the archive, it stops processing at the first zero header.
    let actual = decode_names(&mut Archive::new(random_cursor_reader(&archive_bytes)));
    assert_eq!(expected, actual);

    // extend expected by itself.
    let expected: Vec<&str> = {
        let mut o = Vec::new();
        o.extend(&expected);
        o.extend(&expected);
        o
    };

    let mut ar = Archive::new(random_cursor_reader(&archive_bytes));
    ar.set_ignore_zeros(true);

    let actual = decode_names(&mut ar);
    assert_eq!(expected, actual);

    fn decode_names<R>(ar: &mut Archive<R>) -> Vec<String>
    where
        R: Read,
    {
        let mut names = Vec::new();

        for entry in ar.entries().unwrap() {
            let e = entry.unwrap();
            names.push(::std::str::from_utf8(&e.path_bytes()).unwrap().to_string());
        }

        names
    }
}

#[test]
fn header_impls() {
    let mut ar = Archive::new(random_cursor_reader(tar!("simple.tar")));
    let hn = Header::new_old();
    let hnb = hn.as_bytes();
    for file in ar.entries().unwrap() {
        let file = file.unwrap();
        let h1 = file.header();
        let h1b = h1.as_bytes();
        let h2 = h1.clone();
        let h2b = h2.as_bytes();
        assert!(h1b[..] == h2b[..] && h2b[..] != hnb[..])
    }
}

#[test]
fn header_impls_missing_last_header() {
    let mut ar = Archive::new(random_cursor_reader(tar!("simple_missing_last_header.tar")));
    let hn = Header::new_old();
    let hnb = hn.as_bytes();
    for file in ar.entries().unwrap() {
        let file = file.unwrap();
        let h1 = file.header();
        let h1b = h1.as_bytes();
        let h2 = h1.clone();
        let h2b = h2.as_bytes();
        assert!(h1b[..] == h2b[..] && h2b[..] != hnb[..])
    }
}

#[test]
fn reading_files() {
    let rdr = random_cursor_reader(tar!("reading_files.tar"));
    let mut ar = Archive::new(rdr);
    let mut entries = ar.entries().unwrap();

    let mut a = entries.next().unwrap().unwrap();
    assert_eq!(&*a.header().path_bytes(), b"a");
    let mut s = String::new();
    a.read_to_string(&mut s).unwrap();
    assert_eq!(s, "a\na\na\na\na\na\na\na\na\na\na\n");

    let mut b = entries.next().unwrap().unwrap();
    assert_eq!(&*b.header().path_bytes(), b"b");
    s.truncate(0);
    b.read_to_string(&mut s).unwrap();
    assert_eq!(s, "b\nb\nb\nb\nb\nb\nb\nb\nb\nb\nb\n");

    assert!(entries.next().is_none());
}

#[test]
fn writing_files() {
    let mut ar = Builder::new(Vec::new());
    let td = TempBuilder::new().prefix("tar-rs").tempdir().unwrap();

    let path = td.path().join("test");
    File::create(&path).unwrap().write_all(b"test").unwrap();

    ar.append_file("test2", &mut File::open(&path).unwrap())
        .unwrap();

    let data = ar.into_inner().unwrap();
    let mut ar = Archive::new(Cursor::new(data));
    let mut entries = ar.entries().unwrap();
    let mut f = entries.next().unwrap().unwrap();

    assert_eq!(&*f.header().path_bytes(), b"test2");
    assert_eq!(f.header().size().unwrap(), 4);
    let mut s = String::new();
    f.read_to_string(&mut s).unwrap();
    assert_eq!(s, "test");

    assert!(entries.next().is_none());
}

#[test]
fn large_filename() {
    let mut ar = Builder::new(Vec::new());
    let td = TempBuilder::new().prefix("tar-rs").tempdir().unwrap();

    let path = td.path().join("test");
    File::create(&path).unwrap().write_all(b"test").unwrap();

    let filename = "abcd/".repeat(50);
    let mut header = Header::new_ustar();
    header.set_path(&filename).unwrap();
    header.set_metadata(&fs::metadata(&path).unwrap());
    header.set_cksum();
    ar.append(&header, &b"test"[..]).unwrap();
    let too_long = "abcd".repeat(200);
    ar.append_file(&too_long, &mut File::open(&path).unwrap())
        .unwrap();
    ar.append_data(&mut header, &too_long, &b"test"[..])
        .unwrap();

    let rd = Cursor::new(ar.into_inner().unwrap());
    let mut ar = Archive::new(rd);
    let mut entries = ar.entries().unwrap();

    // The short entry added with `append`
    let mut f = entries.next().unwrap().unwrap();
    assert_eq!(&*f.header().path_bytes(), filename.as_bytes());
    assert_eq!(f.header().size().unwrap(), 4);
    let mut s = String::new();
    f.read_to_string(&mut s).unwrap();
    assert_eq!(s, "test");

    // The long entry added with `append_file`
    let mut f = entries.next().unwrap().unwrap();
    assert_eq!(&*f.path_bytes(), too_long.as_bytes());
    assert_eq!(f.header().size().unwrap(), 4);
    let mut s = String::new();
    f.read_to_string(&mut s).unwrap();
    assert_eq!(s, "test");

    // The long entry added with `append_data`
    let mut f = entries.next().unwrap().unwrap();
    assert!(f.header().path_bytes().len() < too_long.len());
    assert_eq!(&*f.path_bytes(), too_long.as_bytes());
    assert_eq!(f.header().size().unwrap(), 4);
    let mut s = String::new();
    f.read_to_string(&mut s).unwrap();
    assert_eq!(s, "test");

    assert!(entries.next().is_none());
}

// This test checks very particular scenario where a path component starting
// with ".." of a long path gets split at 100-byte mark so that ".." part goes
// into header and gets interpreted as parent dir (and rejected) .
#[test]
fn large_filename_with_dot_dot_at_100_byte_mark() {
    let mut ar = Builder::new(Vec::new());

    let mut header = Header::new_gnu();
    header.set_mode(0o644);
    header.set_size(4);

    let mut long_name_with_dot_dot = "tdir/".repeat(19);
    long_name_with_dot_dot.push_str("tt/..file");

    ar.append_data(&mut header, &long_name_with_dot_dot, b"test".as_slice())
        .unwrap();

    let rd = Cursor::new(ar.into_inner().unwrap());
    let mut ar = Archive::new(rd);
    let mut entries = ar.entries().unwrap();

    let mut f = entries.next().unwrap().unwrap();
    assert_eq!(&*f.path_bytes(), long_name_with_dot_dot.as_bytes());
    assert_eq!(f.header().size().unwrap(), 4);
    let mut s = String::new();
    f.read_to_string(&mut s).unwrap();
    assert_eq!(s, "test");
    assert!(entries.next().is_none());
}

fn reading_entries_common<R: Read>(mut entries: Entries<R>) {
    let mut a = entries.next().unwrap().unwrap();
    assert_eq!(&*a.header().path_bytes(), b"a");
    let mut s = String::new();
    a.read_to_string(&mut s).unwrap();
    assert_eq!(s, "a\na\na\na\na\na\na\na\na\na\na\n");
    s.truncate(0);
    a.read_to_string(&mut s).unwrap();
    assert_eq!(s, "");

    let mut b = entries.next().unwrap().unwrap();
    assert_eq!(&*b.header().path_bytes(), b"b");
    s.truncate(0);
    b.read_to_string(&mut s).unwrap();
    assert_eq!(s, "b\nb\nb\nb\nb\nb\nb\nb\nb\nb\nb\n");
    assert!(entries.next().is_none());
}

#[test]
fn reading_entries() {
    let rdr = random_cursor_reader(tar!("reading_files.tar"));
    let mut ar = Archive::new(rdr);
    reading_entries_common(ar.entries().unwrap());
}

#[test]
fn reading_entries_with_seek() {
    let rdr = Cursor::new(tar!("reading_files.tar"));
    let mut ar = Archive::new(rdr);
    reading_entries_common(ar.entries_with_seek().unwrap());
}

struct LoggingReader<R> {
    inner: R,
    read_bytes: u64,
}

impl<R> LoggingReader<R> {
    fn new(reader: R) -> LoggingReader<R> {
        LoggingReader {
            inner: reader,
            read_bytes: 0,
        }
    }
}

impl<T: Read> Read for LoggingReader<T> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf).inspect(|&i| {
            self.read_bytes += i as u64;
        })
    }
}

impl<T: Seek> Seek for LoggingReader<T> {
    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        self.inner.seek(pos)
    }
}

#[test]
fn skipping_entries_with_seek() {
    let mut reader = LoggingReader::new(Cursor::new(tar!("reading_files.tar")));
    let mut ar_reader = Archive::new(&mut reader);
    let files: Vec<_> = ar_reader
        .entries()
        .unwrap()
        .map(|entry| entry.unwrap().path().unwrap().to_path_buf())
        .collect();

    let mut seekable_reader = LoggingReader::new(Cursor::new(tar!("reading_files.tar")));
    let mut ar_seekable_reader = Archive::new(&mut seekable_reader);
    let files_seekable: Vec<_> = ar_seekable_reader
        .entries_with_seek()
        .unwrap()
        .map(|entry| entry.unwrap().path().unwrap().to_path_buf())
        .collect();

    assert!(files == files_seekable);
    assert!(seekable_reader.read_bytes < reader.read_bytes);
}

fn check_dirtree(td: &TempDir) {
    let dir_a = td.path().join("a");
    let dir_b = td.path().join("a/b");
    let file_c = td.path().join("a/c");
    assert!(fs::metadata(&dir_a).map(|m| m.is_dir()).unwrap_or(false));
    assert!(fs::metadata(&dir_b).map(|m| m.is_dir()).unwrap_or(false));
    assert!(fs::metadata(&file_c).map(|m| m.is_file()).unwrap_or(false));
}

#[test]
fn extracting_directories() {
    let td = TempBuilder::new().prefix("tar-rs").tempdir().unwrap();
    let rdr = random_cursor_reader(tar!("directory.tar"));
    let mut ar = Archive::new(rdr);
    ar.unpack(td.path()).unwrap();
    check_dirtree(&td);
}

#[test]
fn extracting_duplicate_file_fail() {
    let td = TempBuilder::new().prefix("tar-rs").tempdir().unwrap();
    let path_present = td.path().join("a");
    File::create(path_present).unwrap();

    let rdr = random_cursor_reader(tar!("reading_files.tar"));
    let mut ar = Archive::new(rdr);
    ar.set_overwrite(false);
    if let Err(err) = ar.unpack(td.path()) {
        if err.kind() == std::io::ErrorKind::AlreadyExists {
            // as expected with overwrite false
            return;
        }
        panic!("unexpected error: {:?}", err);
    }
    panic!(
        "unpack() should have returned an error of kind {:?}, returned Ok",
        std::io::ErrorKind::AlreadyExists
    )
}

#[test]
fn extracting_duplicate_file_succeed() {
    let td = TempBuilder::new().prefix("tar-rs").tempdir().unwrap();
    let path_present = td.path().join("a");
    File::create(path_present).unwrap();

    let rdr = random_cursor_reader(tar!("reading_files.tar"));
    let mut ar = Archive::new(rdr);
    ar.set_overwrite(true);
    ar.unpack(td.path()).unwrap();
}

#[test]
#[cfg(unix)]
fn extracting_duplicate_link_fail() {
    let td = TempBuilder::new().prefix("tar-rs").tempdir().unwrap();
    let path_present = td.path().join("lnk");
    std::os::unix::fs::symlink("file", path_present).unwrap();

    let rdr = random_cursor_reader(tar!("link.tar"));
    let mut ar = Archive::new(rdr);
    ar.set_overwrite(false);
    if let Err(err) = ar.unpack(td.path()) {
        if err.kind() == std::io::ErrorKind::AlreadyExists {
            // as expected with overwrite false
            return;
        }
        panic!("unexpected error: {:?}", err);
    }
    panic!(
        "unpack() should have returned an error of kind {:?}, returned Ok",
        std::io::ErrorKind::AlreadyExists
    )
}

#[test]
#[cfg(unix)]
fn extracting_duplicate_link_succeed() {
    let td = TempBuilder::new().prefix("tar-rs").tempdir().unwrap();
    let path_present = td.path().join("lnk");
    std::os::unix::fs::symlink("file", path_present).unwrap();

    let rdr = random_cursor_reader(tar!("link.tar"));
    let mut ar = Archive::new(rdr);
    ar.set_overwrite(true);
    ar.unpack(td.path()).unwrap();
}

#[test]
#[cfg(all(unix, feature = "xattr"))]
fn xattrs() {
    // If /tmp is a tmpfs, xattr will fail
    // The xattr crate's unit tests also use /var/tmp for this reason
    let td = TempBuilder::new()
        .prefix("tar-rs")
        .tempdir_in("/var/tmp")
        .unwrap();
    let rdr = random_cursor_reader(tar!("xattrs.tar"));
    let mut ar = Archive::new(rdr);
    ar.set_unpack_xattrs(true);
    ar.unpack(td.path()).unwrap();

    let val = xattr::get(td.path().join("a/b"), "user.pax.flags").unwrap();
    assert_eq!(val.unwrap(), "epm".as_bytes());
}

#[test]
#[cfg(all(unix, feature = "xattr"))]
fn no_xattrs() {
    // If /tmp is a tmpfs, xattr will fail
    // The xattr crate's unit tests also use /var/tmp for this reason
    let td = TempBuilder::new()
        .prefix("tar-rs")
        .tempdir_in("/var/tmp")
        .unwrap();
    let rdr = random_cursor_reader(tar!("xattrs.tar"));
    let mut ar = Archive::new(rdr);
    ar.set_unpack_xattrs(false);
    ar.unpack(td.path()).unwrap();

    assert_eq!(
        xattr::get(td.path().join("a/b"), "user.pax.flags").unwrap(),
        None
    );
}

#[test]
fn writing_and_extracting_directories() {
    let td = TempBuilder::new().prefix("tar-rs").tempdir().unwrap();

    let mut ar = Builder::new(Vec::new());
    let tmppath = td.path().join("tmpfile");
    File::create(&tmppath).unwrap().write_all(b"c").unwrap();
    ar.append_dir("a", ".").unwrap();
    ar.append_dir("a/b", ".").unwrap();
    ar.append_file("a/c", &mut File::open(&tmppath).unwrap())
        .unwrap();
    ar.finish().unwrap();

    let rdr = Cursor::new(ar.into_inner().unwrap());
    let mut ar = Archive::new(rdr);
    ar.unpack(td.path()).unwrap();
    check_dirtree(&td);
}

#[test]
fn writing_files_absolute_path_fail() {
    let td = TempBuilder::new().prefix("tar-rs").tempdir().unwrap();
    let mut ar = Builder::new(Vec::new());

    let td_abs_path = td.path().to_path_buf();
    if let Err(res) = ar.append_dir(&td_abs_path, &td_abs_path) {
        assert!(res
            .to_string()
            .contains("paths in archives must be relative when setting path for"));
        return;
    }

    panic!("Expected error");
}

#[test]
fn writing_files_absolute_path_succeed() {
    let td = TempBuilder::new().prefix("tar-rs").tempdir().unwrap();

    let mut ar = Builder::new(Vec::new());
    ar.preserve_absolute(true);

    let td_abs_path = td.path().to_path_buf();
    ar.append_dir(&td_abs_path, &td_abs_path).unwrap();
    ar.finish().unwrap();

    let rdr = Cursor::new(ar.into_inner().unwrap());
    let mut ar = Archive::new(rdr);
    let actual: Vec<PathBuf> = ar
        .entries()
        .unwrap()
        .map(|e| e.unwrap().path().unwrap().into_owned())
        .collect();

    assert_eq!(actual, vec![td_abs_path]);
}

#[test]
fn extract_absolute_path_gnu_tar() {
    let td_abs_path = TempBuilder::new().prefix("tar-rs").tempdir().unwrap();

    let test_file = td_abs_path.path().join("tmpfile");
    File::create(&test_file)
        .unwrap()
        .write_all(b"content")
        .unwrap();

    let test_arr = td_abs_path.path().join("arr.tar");

    Command::new("tar")
        .args([
            "-cf",
            &test_arr.display().to_string(),
            "-P",
            &test_file.display().to_string(),
        ])
        .status()
        .expect("Failed to create an archive via GNU tar");

    assert!(fs::metadata(&test_arr).is_ok());

    fs::remove_file(&test_file).unwrap();
    assert!(fs::metadata(&test_file).is_err());

    let mut ar = Archive::new(File::open(&test_arr).unwrap());
    ar.unpack(&td_abs_path).unwrap();

    let unpacked_path = td_abs_path.path().join(
        test_file
            .components()
            .skip_while(|c| matches!(c, Component::RootDir | Component::Prefix(_)))
            .collect::<PathBuf>(),
    );
    assert!(fs::metadata(&unpacked_path).is_ok());
}

#[test]
fn writing_and_extracting_directories_complex_permissions() {
    let td = TempBuilder::new().prefix("tar-rs").tempdir().unwrap();

    // Archive with complex permissions which would fail to unpack if one attempted to do so
    // without reordering of entries.
    let mut ar = Builder::new(Vec::new());
    let tmppath = td.path().join("tmpfile");
    File::create(&tmppath).unwrap().write_all(b"c").unwrap();

    // Root dir with very stringent permissions
    let data: &[u8] = &[];
    let mut header = Header::new_gnu();
    header.set_mode(0o555);
    header.set_entry_type(EntryType::Directory);
    header.set_path("a").unwrap();
    header.set_size(0);
    header.set_cksum();
    ar.append(&header, data).unwrap();

    // Nested dir
    header.set_mode(0o777);
    header.set_entry_type(EntryType::Directory);
    header.set_path("a/b").unwrap();
    header.set_cksum();
    ar.append(&header, data).unwrap();

    // Nested file.
    ar.append_file("a/c", &mut File::open(&tmppath).unwrap())
        .unwrap();
    ar.finish().unwrap();

    let rdr = Cursor::new(ar.into_inner().unwrap());
    let mut ar = Archive::new(rdr);
    ar.unpack(td.path()).unwrap();
    check_dirtree(&td);
}

#[test]
fn writing_directories_recursively() {
    let td = TempBuilder::new().prefix("tar-rs").tempdir().unwrap();

    let base_dir = td.path().join("base");
    fs::create_dir(&base_dir).unwrap();
    File::create(base_dir.join("file1"))
        .unwrap()
        .write_all(b"file1")
        .unwrap();
    let sub_dir = base_dir.join("sub");
    fs::create_dir(&sub_dir).unwrap();
    File::create(sub_dir.join("file2"))
        .unwrap()
        .write_all(b"file2")
        .unwrap();

    let mut ar = Builder::new(Vec::new());
    ar.append_dir_all("foobar", base_dir).unwrap();
    let data = ar.into_inner().unwrap();

    let mut ar = Archive::new(Cursor::new(data));
    ar.unpack(td.path()).unwrap();
    let base_dir = td.path().join("foobar");
    assert!(fs::metadata(&base_dir).map(|m| m.is_dir()).unwrap_or(false));
    let file1_path = base_dir.join("file1");
    assert!(fs::metadata(&file1_path)
        .map(|m| m.is_file())
        .unwrap_or(false));
    let sub_dir = base_dir.join("sub");
    assert!(fs::metadata(&sub_dir).map(|m| m.is_dir()).unwrap_or(false));
    let file2_path = sub_dir.join("file2");
    assert!(fs::metadata(&file2_path)
        .map(|m| m.is_file())
        .unwrap_or(false));
}

#[test]
fn append_dir_all_blank_dest() {
    let td = TempBuilder::new().prefix("tar-rs").tempdir().unwrap();

    let base_dir = td.path().join("base");
    fs::create_dir(&base_dir).unwrap();
    File::create(base_dir.join("file1"))
        .unwrap()
        .write_all(b"file1")
        .unwrap();
    let sub_dir = base_dir.join("sub");
    fs::create_dir(&sub_dir).unwrap();
    File::create(sub_dir.join("file2"))
        .unwrap()
        .write_all(b"file2")
        .unwrap();

    let mut ar = Builder::new(Vec::new());
    ar.append_dir_all("", base_dir).unwrap();
    let data = ar.into_inner().unwrap();

    let mut ar = Archive::new(Cursor::new(data));
    ar.unpack(td.path()).unwrap();
    let base_dir = td.path();
    assert!(fs::metadata(base_dir).map(|m| m.is_dir()).unwrap_or(false));
    let file1_path = base_dir.join("file1");
    assert!(fs::metadata(&file1_path)
        .map(|m| m.is_file())
        .unwrap_or(false));
    let sub_dir = base_dir.join("sub");
    assert!(fs::metadata(&sub_dir).map(|m| m.is_dir()).unwrap_or(false));
    let file2_path = sub_dir.join("file2");
    assert!(fs::metadata(&file2_path)
        .map(|m| m.is_file())
        .unwrap_or(false));
}

#[test]
fn append_dir_all_does_not_work_on_non_directory() {
    let td = TempBuilder::new().prefix("tar-rs").tempdir().unwrap();
    let path = td.path().join("test");
    File::create(&path).unwrap().write_all(b"test").unwrap();

    let mut ar = Builder::new(Vec::new());
    let result = ar.append_dir_all("test", path);
    assert!(result.is_err());
}

#[test]
fn extracting_duplicate_dirs() {
    let td = TempBuilder::new().prefix("tar-rs").tempdir().unwrap();
    let rdr = random_cursor_reader(tar!("duplicate_dirs.tar"));
    let mut ar = Archive::new(rdr);
    ar.unpack(td.path()).unwrap();

    let some_dir = td.path().join("some_dir");
    assert!(fs::metadata(&some_dir).map(|m| m.is_dir()).unwrap_or(false));
}

#[test]
fn unpack_old_style_bsd_dir() {
    let td = TempBuilder::new().prefix("tar-rs").tempdir().unwrap();

    let mut ar = Builder::new(Vec::new());

    let mut header = Header::new_old();
    header.set_entry_type(EntryType::Regular);
    header.set_path("testdir/").unwrap();
    header.set_size(0);
    header.set_cksum();
    ar.append(&header, &mut io::empty()).unwrap();

    // Extracting
    let rdr = Cursor::new(ar.into_inner().unwrap());
    let mut ar = Archive::new(rdr);
    ar.unpack(td.path()).unwrap();

    // Iterating
    let rdr = Cursor::new(ar.into_inner().into_inner());
    let mut ar = Archive::new(rdr);
    assert!(ar.entries().unwrap().all(|fr| fr.is_ok()));

    assert!(td.path().join("testdir").is_dir());
}

#[test]
fn handling_incorrect_file_size() {
    let td = TempBuilder::new().prefix("tar-rs").tempdir().unwrap();

    let mut ar = Builder::new(Vec::new());

    let path = td.path().join("tmpfile");
    File::create(&path).unwrap();
    let mut file = File::open(&path).unwrap();
    let mut header = Header::new_old();
    header.set_path("somepath").unwrap();
    header.set_metadata(&file.metadata().unwrap());
    header.set_size(2048); // past the end of file null blocks
    header.set_cksum();
    ar.append(&header, &mut file).unwrap();

    // Extracting
    let rdr = Cursor::new(ar.into_inner().unwrap());
    let mut ar = Archive::new(rdr);
    assert!(ar.unpack(td.path()).is_err());

    // Iterating
    let rdr = Cursor::new(ar.into_inner().into_inner());
    let mut ar = Archive::new(rdr);
    assert!(ar.entries().unwrap().any(|fr| fr.is_err()));
}

#[test]
fn extracting_malicious_tarball() {
    let td = TempBuilder::new().prefix("tar-rs").tempdir().unwrap();

    let mut evil_tar = Vec::new();

    {
        let mut a = Builder::new(&mut evil_tar);
        let mut append = |path: &str| {
            let mut header = Header::new_gnu();
            assert!(header.set_path(path).is_err(), "was ok: {:?}", path);
            {
                let h = header.as_gnu_mut().unwrap();
                for (a, b) in h.name.iter_mut().zip(path.as_bytes()) {
                    *a = *b;
                }
            }
            header.set_size(1);
            header.set_cksum();
            a.append(&header, io::repeat(1).take(1)).unwrap();
        };
        append("/tmp/abs_evil.txt");
        // std parse `//` as UNC path, see rust-lang/rust#100833
        append(
            #[cfg(not(windows))]
            "//tmp/abs_evil2.txt",
            #[cfg(windows)]
            "C://tmp/abs_evil2.txt",
        );
        append("///tmp/abs_evil3.txt");
        append("/./tmp/abs_evil4.txt");
        append(
            #[cfg(not(windows))]
            "//./tmp/abs_evil5.txt",
            #[cfg(windows)]
            "C://./tmp/abs_evil5.txt",
        );
        append("///./tmp/abs_evil6.txt");
        append("/../tmp/rel_evil.txt");
        append("../rel_evil2.txt");
        append("./../rel_evil3.txt");
        append("some/../../rel_evil4.txt");
        append("");
        append("././//./..");
        append("..");
        append("/////////..");
        append("/////////");
    }

    let mut ar = Archive::new(&evil_tar[..]);
    ar.unpack(td.path()).unwrap();

    assert!(fs::metadata("/tmp/abs_evil.txt").is_err());
    assert!(fs::metadata("/tmp/abs_evil.txt2").is_err());
    assert!(fs::metadata("/tmp/abs_evil.txt3").is_err());
    assert!(fs::metadata("/tmp/abs_evil.txt4").is_err());
    assert!(fs::metadata("/tmp/abs_evil.txt5").is_err());
    assert!(fs::metadata("/tmp/abs_evil.txt6").is_err());
    assert!(fs::metadata("/tmp/rel_evil.txt").is_err());
    assert!(fs::metadata("/tmp/rel_evil.txt").is_err());
    assert!(fs::metadata(td.path().join("../tmp/rel_evil.txt")).is_err());
    assert!(fs::metadata(td.path().join("../rel_evil2.txt")).is_err());
    assert!(fs::metadata(td.path().join("../rel_evil3.txt")).is_err());
    assert!(fs::metadata(td.path().join("../rel_evil4.txt")).is_err());

    // The `some` subdirectory should not be created because the only
    // filename that references this has '..'.
    assert!(fs::metadata(td.path().join("some")).is_err());

    // The `tmp` subdirectory should be created and within this
    // subdirectory, there should be files named `abs_evil.txt` through
    // `abs_evil6.txt`.
    assert!(fs::metadata(td.path().join("tmp"))
        .map(|m| m.is_dir())
        .unwrap_or(false));
    assert!(fs::metadata(td.path().join("tmp/abs_evil.txt"))
        .map(|m| m.is_file())
        .unwrap_or(false));
    assert!(fs::metadata(td.path().join("tmp/abs_evil2.txt"))
        .map(|m| m.is_file())
        .unwrap_or(false));
    assert!(fs::metadata(td.path().join("tmp/abs_evil3.txt"))
        .map(|m| m.is_file())
        .unwrap_or(false));
    assert!(fs::metadata(td.path().join("tmp/abs_evil4.txt"))
        .map(|m| m.is_file())
        .unwrap_or(false));
    assert!(fs::metadata(td.path().join("tmp/abs_evil5.txt"))
        .map(|m| m.is_file())
        .unwrap_or(false));
    assert!(fs::metadata(td.path().join("tmp/abs_evil6.txt"))
        .map(|m| m.is_file())
        .unwrap_or(false));
}

#[test]
fn octal_spaces() {
    let rdr = random_cursor_reader(tar!("spaces.tar"));
    let mut ar = Archive::new(rdr);

    let entry = ar.entries().unwrap().next().unwrap().unwrap();
    assert_eq!(entry.header().mode().unwrap() & 0o777, 0o777);
    assert_eq!(entry.header().uid().unwrap(), 0);
    assert_eq!(entry.header().gid().unwrap(), 0);
    assert_eq!(entry.header().size().unwrap(), 2);
    assert_eq!(entry.header().mtime().unwrap(), 0o12440016664);
    assert_eq!(entry.header().cksum().unwrap(), 0o4253);
}

#[test]
fn extracting_malformed_tar_null_blocks() {
    let td = TempBuilder::new().prefix("tar-rs").tempdir().unwrap();

    let mut ar = Builder::new(Vec::new());

    let path1 = td.path().join("tmpfile1");
    let path2 = td.path().join("tmpfile2");
    File::create(&path1).unwrap();
    File::create(&path2).unwrap();
    ar.append_file("tmpfile1", &mut File::open(&path1).unwrap())
        .unwrap();
    let mut data = ar.into_inner().unwrap();
    let amt = data.len();
    data.truncate(amt - 512);
    let mut ar = Builder::new(data);
    ar.append_file("tmpfile2", &mut File::open(&path2).unwrap())
        .unwrap();
    ar.finish().unwrap();

    let data = ar.into_inner().unwrap();
    let mut ar = Archive::new(&data[..]);
    assert!(ar.unpack(td.path()).is_ok());
}

#[test]
fn empty_filename() {
    let td = TempBuilder::new().prefix("tar-rs").tempdir().unwrap();
    let rdr = random_cursor_reader(tar!("empty_filename.tar"));
    let mut ar = Archive::new(rdr);
    assert!(ar.unpack(td.path()).is_ok());
}

#[test]
fn file_times() {
    let td = TempBuilder::new().prefix("tar-rs").tempdir().unwrap();
    let rdr = random_cursor_reader(tar!("file_times.tar"));
    let mut ar = Archive::new(rdr);
    ar.unpack(td.path()).unwrap();

    let meta = fs::metadata(td.path().join("a")).unwrap();
    let mtime = FileTime::from_last_modification_time(&meta);
    let atime = FileTime::from_last_access_time(&meta);
    assert_eq!(mtime.unix_seconds(), 1000000000);
    assert_eq!(mtime.nanoseconds(), 0);
    assert_eq!(atime.unix_seconds(), 1000000000);
    assert_eq!(atime.nanoseconds(), 0);
}

#[test]
fn zero_file_times() {
    let td = TempBuilder::new().prefix("tar-rs").tempdir().unwrap();

    let mut ar = Builder::new(Vec::new());
    ar.mode(HeaderMode::Deterministic);
    let path = td.path().join("tmpfile");
    File::create(&path).unwrap();
    ar.append_path_with_name(&path, "a").unwrap();

    let data = ar.into_inner().unwrap();
    let mut ar = Archive::new(&data[..]);
    assert!(ar.unpack(td.path()).is_ok());

    let meta = fs::metadata(td.path().join("a")).unwrap();
    let mtime = FileTime::from_last_modification_time(&meta);
    let atime = FileTime::from_last_access_time(&meta);
    assert!(mtime.unix_seconds() != 0);
    assert!(atime.unix_seconds() != 0);
}

#[test]
fn backslash_treated_well() {
    // Insert a file into an archive with a backslash
    let td = TempBuilder::new().prefix("tar-rs").tempdir().unwrap();
    let mut ar = Builder::new(Vec::<u8>::new());
    ar.append_dir("foo\\bar", td.path()).unwrap();
    let mut ar = Archive::new(Cursor::new(ar.into_inner().unwrap()));
    let f = ar.entries().unwrap().next().unwrap().unwrap();
    if cfg!(unix) {
        assert_eq!(f.header().path().unwrap().to_str(), Some("foo\\bar"));
    } else {
        assert_eq!(f.header().path().unwrap().to_str(), Some("foo/bar"));
    }

    // Unpack an archive with a backslash in the name
    let mut ar = Builder::new(Vec::<u8>::new());
    let mut header = Header::new_gnu();
    header.set_metadata(&fs::metadata(td.path()).unwrap());
    header.set_size(0);
    for (a, b) in header.as_old_mut().name.iter_mut().zip(b"foo\\bar\x00") {
        *a = *b;
    }
    header.set_cksum();
    ar.append(&header, &mut io::empty()).unwrap();
    let data = ar.into_inner().unwrap();
    let mut ar = Archive::new(&data[..]);
    let f = ar.entries().unwrap().next().unwrap().unwrap();
    assert_eq!(f.header().path().unwrap().to_str(), Some("foo\\bar"));

    let mut ar = Archive::new(&data[..]);
    ar.unpack(td.path()).unwrap();
    assert!(fs::metadata(td.path().join("foo\\bar")).is_ok());
}

#[test]
#[cfg(unix)]
fn set_mask() {
    use std::os::unix::fs::PermissionsExt;
    let mut ar = tar::Builder::new(Vec::new());

    let mut header = tar::Header::new_gnu();
    header.set_size(0);
    header.set_entry_type(tar::EntryType::Regular);
    header.set_path("foo").unwrap();
    header.set_mode(0o777);
    header.set_cksum();
    ar.append(&header, &[][..]).unwrap();

    let mut header = tar::Header::new_gnu();
    header.set_size(0);
    header.set_entry_type(tar::EntryType::Regular);
    header.set_path("bar").unwrap();
    header.set_mode(0o421);
    header.set_cksum();
    ar.append(&header, &[][..]).unwrap();

    let td = TempBuilder::new().prefix("tar-rs").tempdir().unwrap();
    let bytes = ar.into_inner().unwrap();
    let mut ar = tar::Archive::new(&bytes[..]);
    ar.set_mask(0o211);
    ar.unpack(td.path()).unwrap();

    let md = fs::metadata(td.path().join("foo")).unwrap();
    assert_eq!(md.permissions().mode(), 0o100566);
    let md = fs::metadata(td.path().join("bar")).unwrap();
    assert_eq!(md.permissions().mode(), 0o100420);
}

#[cfg(unix)]
#[test]
fn nul_bytes_in_path() {
    use std::ffi::OsStr;
    use std::os::unix::prelude::*;

    let nul_path = OsStr::from_bytes(b"foo\0");
    let td = TempBuilder::new().prefix("tar-rs").tempdir().unwrap();
    let mut ar = Builder::new(Vec::<u8>::new());
    let err = ar.append_dir(nul_path, td.path()).unwrap_err();
    assert!(err.to_string().contains("contains a nul byte"));
}

#[test]
fn links() {
    let mut ar = Archive::new(random_cursor_reader(tar!("link.tar")));
    let mut entries = ar.entries().unwrap();
    let link = entries.next().unwrap().unwrap();
    assert_eq!(
        link.header().link_name().unwrap().as_deref(),
        Some(Path::new("file"))
    );
    let other = entries.next().unwrap().unwrap();
    assert!(other.header().link_name().unwrap().is_none());
}

#[test]
#[cfg(unix)] // making symlinks on windows is hard
fn unpack_links() {
    let td = TempBuilder::new().prefix("tar-rs").tempdir().unwrap();
    let mut ar = Archive::new(random_cursor_reader(tar!("link.tar")));
    ar.unpack(td.path()).unwrap();

    let md = fs::symlink_metadata(td.path().join("lnk")).unwrap();
    assert!(md.file_type().is_symlink());

    let mtime = FileTime::from_last_modification_time(&md);
    assert_eq!(mtime.unix_seconds(), 1448291033);

    assert_eq!(
        &*fs::read_link(td.path().join("lnk")).unwrap(),
        Path::new("file")
    );
    File::open(td.path().join("lnk")).unwrap();
}

#[test]
fn pax_size() {
    let mut ar = Archive::new(random_cursor_reader(tar!("pax_size.tar")));
    let mut entries = ar.entries().unwrap();
    let mut entry = entries.next().unwrap().unwrap();
    let mut attributes = entry.pax_extensions().unwrap().unwrap();

    let _first = attributes.next().unwrap().unwrap();
    let _second = attributes.next().unwrap().unwrap();
    let _third = attributes.next().unwrap().unwrap();
    let fourth = attributes.next().unwrap().unwrap();
    assert!(attributes.next().is_none());

    assert_eq!(fourth.key(), Ok("size"));
    assert_eq!(fourth.value(), Ok("4"));

    assert_eq!(entry.header().size().unwrap(), 0);
    assert_eq!(entry.size(), 4);
}

#[test]
fn pax_simple() {
    let mut ar = Archive::new(random_cursor_reader(tar!("pax.tar")));
    let mut entries = ar.entries().unwrap();

    let mut first = entries.next().unwrap().unwrap();
    let mut attributes = first.pax_extensions().unwrap().unwrap();
    let first = attributes.next().unwrap().unwrap();
    let second = attributes.next().unwrap().unwrap();
    let third = attributes.next().unwrap().unwrap();
    assert!(attributes.next().is_none());

    assert_eq!(first.key(), Ok("mtime"));
    assert_eq!(first.value(), Ok("1453146164.953123768"));
    assert_eq!(second.key(), Ok("atime"));
    assert_eq!(second.value(), Ok("1453251915.24892486"));
    assert_eq!(third.key(), Ok("ctime"));
    assert_eq!(third.value(), Ok("1453146164.953123768"));
}

#[test]
fn pax_simple_write() {
    let td = TempBuilder::new().prefix("tar-rs").tempdir().unwrap();
    let pax_path = td.path().join("pax.tar");
    let file: File = File::create(&pax_path).unwrap();
    let mut ar: Builder<BufWriter<File>> = Builder::new(BufWriter::new(file));

    let pax_extensions = [
        ("arbitrary_pax_key", b"arbitrary_pax_value".as_slice()),
        ("SCHILY.xattr.security.selinux", b"foo_t"),
    ];

    ar.append_pax_extensions(pax_extensions).unwrap();
    ar.append_file("test2", &mut File::open(&pax_path).unwrap())
        .unwrap();
    ar.finish().unwrap();
    drop(ar);

    let mut archive_opened = Archive::new(File::open(pax_path).unwrap());
    let mut entries = archive_opened.entries().unwrap();
    let mut f: Entry<File> = entries.next().unwrap().unwrap();
    let pax_headers = f.pax_extensions().unwrap();

    assert!(pax_headers.is_some(), "pax_headers is None");
    let mut pax_headers = pax_headers.unwrap();
    let pax_arbitrary = pax_headers.next().unwrap().unwrap();
    assert_eq!(pax_arbitrary.key(), Ok("arbitrary_pax_key"));
    assert_eq!(pax_arbitrary.value(), Ok("arbitrary_pax_value"));
    let xattr = pax_headers.next().unwrap().unwrap();
    assert_eq!(xattr.key().unwrap(), pax_extensions[1].0);
    assert_eq!(xattr.value_bytes(), pax_extensions[1].1);

    assert!(entries.next().is_none());
}

#[test]
fn pax_path() {
    let mut ar = Archive::new(random_cursor_reader(tar!("pax2.tar")));
    let mut entries = ar.entries().unwrap();

    let first = entries.next().unwrap().unwrap();
    assert!(first.path().unwrap().ends_with("aaaaaaaaaaaaaaa"));
}

#[test]
fn pax_linkpath() {
    let mut ar = Archive::new(random_cursor_reader(tar!("pax2.tar")));
    let mut links = ar.entries().unwrap().skip(3).take(2);

    let long_symlink = links.next().unwrap().unwrap();
    let link_name = long_symlink.link_name().unwrap().unwrap();
    assert!(link_name.to_str().unwrap().len() > 99);
    assert!(link_name.ends_with("bbbbbbbbbbbbbbb"));

    let long_hardlink = links.next().unwrap().unwrap();
    let link_name = long_hardlink.link_name().unwrap().unwrap();
    assert!(link_name.to_str().unwrap().len() > 99);
    assert!(link_name.ends_with("ccccccccccccccc"));
}

#[test]
fn long_name_trailing_nul() {
    let mut b = Builder::new(Vec::<u8>::new());

    let mut h = Header::new_gnu();
    h.set_path("././@LongLink").unwrap();
    h.set_size(4);
    h.set_entry_type(EntryType::new(b'L'));
    h.set_cksum();
    b.append(&h, "foo\0".as_bytes()).unwrap();
    let mut h = Header::new_gnu();

    h.set_path("bar").unwrap();
    h.set_size(6);
    h.set_entry_type(EntryType::file());
    h.set_cksum();
    b.append(&h, "foobar".as_bytes()).unwrap();

    let contents = b.into_inner().unwrap();
    let mut a = Archive::new(&contents[..]);

    let e = a.entries().unwrap().next().unwrap().unwrap();
    assert_eq!(&*e.path_bytes(), b"foo");
}

#[test]
fn long_linkname_trailing_nul() {
    let mut b = Builder::new(Vec::<u8>::new());

    let mut h = Header::new_gnu();
    h.set_path("././@LongLink").unwrap();
    h.set_size(4);
    h.set_entry_type(EntryType::new(b'K'));
    h.set_cksum();
    b.append(&h, "foo\0".as_bytes()).unwrap();
    let mut h = Header::new_gnu();

    h.set_path("bar").unwrap();
    h.set_size(6);
    h.set_entry_type(EntryType::file());
    h.set_cksum();
    b.append(&h, "foobar".as_bytes()).unwrap();

    let contents = b.into_inner().unwrap();
    let mut a = Archive::new(&contents[..]);

    let e = a.entries().unwrap().next().unwrap().unwrap();
    assert_eq!(&*e.link_name_bytes().unwrap(), b"foo");
}

#[test]
fn long_linkname_gnu() {
    for t in [tar::EntryType::Symlink, tar::EntryType::Link] {
        let mut b = Builder::new(Vec::<u8>::new());
        let mut h = Header::new_gnu();
        h.set_entry_type(t);
        h.set_size(0);
        let path = "usr/lib/.build-id/05/159ed904e45ff5100f7acd3d3b99fa7e27e34f";
        let target = "../../../../usr/lib64/qt5/plugins/wayland-graphics-integration-server/libqt-wayland-compositor-xcomposite-egl.so";
        b.append_link(&mut h, path, target).unwrap();

        let contents = b.into_inner().unwrap();
        let mut a = Archive::new(&contents[..]);

        let e = &a.entries().unwrap().next().unwrap().unwrap();
        assert_eq!(e.header().entry_type(), t);
        assert_eq!(e.path().unwrap().to_str().unwrap(), path);
        assert_eq!(e.link_name().unwrap().unwrap().to_str().unwrap(), target);
    }
}

#[test]
fn linkname_literal() {
    for t in [tar::EntryType::Symlink, tar::EntryType::Link] {
        let mut b = Builder::new(Vec::<u8>::new());
        let mut h = Header::new_gnu();
        h.set_entry_type(t);
        h.set_size(0);
        let path = "usr/lib/systemd/systemd-sysv-install";
        let target = "../../..//sbin/chkconfig";
        h.set_link_name_literal(target).unwrap();
        b.append_data(&mut h, path, std::io::empty()).unwrap();

        let contents = b.into_inner().unwrap();
        let mut a = Archive::new(&contents[..]);

        let e = &a.entries().unwrap().next().unwrap().unwrap();
        assert_eq!(e.header().entry_type(), t);
        assert_eq!(e.path().unwrap().to_str().unwrap(), path);
        assert_eq!(e.link_name().unwrap().unwrap().to_str().unwrap(), target);
    }
}

#[test]
fn append_writer() {
    let mut b = Builder::new(Cursor::new(Vec::new()));

    let mut h = Header::new_gnu();
    h.set_uid(42);
    let mut writer = b.append_writer(&mut h, "file1").unwrap();
    writer.write_all(b"foo").unwrap();
    writer.write_all(b"barbaz").unwrap();
    writer.finish().unwrap();

    let mut h = Header::new_gnu();
    h.set_uid(43);
    let long_path: PathBuf = repeat("abcd").take(50).collect();
    let mut writer = b.append_writer(&mut h, &long_path).unwrap();
    let long_data = repeat(b'x').take(513).collect::<Vec<u8>>();
    writer.write_all(&long_data).unwrap();
    writer.finish().unwrap();

    let contents = b.into_inner().unwrap().into_inner();
    let mut ar = Archive::new(&contents[..]);
    let mut entries = ar.entries().unwrap();

    let e = &mut entries.next().unwrap().unwrap();
    assert_eq!(e.header().uid().unwrap(), 42);
    assert_eq!(&*e.path_bytes(), b"file1");
    let mut r = Vec::new();
    e.read_to_end(&mut r).unwrap();
    assert_eq!(&r[..], b"foobarbaz");

    let e = &mut entries.next().unwrap().unwrap();
    assert_eq!(e.header().uid().unwrap(), 43);
    assert_eq!(e.path().unwrap(), long_path.as_path());
    let mut r = Vec::new();
    e.read_to_end(&mut r).unwrap();
    assert_eq!(r.len(), 513);
    assert!(r.iter().all(|b| *b == b'x'));
}

#[test]
fn encoded_long_name_has_trailing_nul() {
    let td = TempBuilder::new().prefix("tar-rs").tempdir().unwrap();
    let path = td.path().join("foo");
    File::create(&path).unwrap().write_all(b"test").unwrap();

    let mut b = Builder::new(Vec::<u8>::new());
    let long = "abcd".repeat(200);

    b.append_file(&long, &mut File::open(&path).unwrap())
        .unwrap();

    let contents = b.into_inner().unwrap();
    let mut a = Archive::new(&contents[..]);

    let mut e = a.entries().unwrap().raw(true).next().unwrap().unwrap();
    let mut name = Vec::new();
    e.read_to_end(&mut name).unwrap();
    assert_eq!(name[name.len() - 1], 0);

    let header_name = &e.header().as_gnu().unwrap().name;
    assert!(header_name.starts_with(b"././@LongLink\x00"));
}

#[test]
fn reading_sparse() {
    let rdr = random_cursor_reader(tar!("sparse.tar"));
    let mut ar = Archive::new(rdr);
    let mut entries = ar.entries().unwrap();

    let mut a = entries.next().unwrap().unwrap();
    let mut s = String::new();
    assert_eq!(&*a.header().path_bytes(), b"sparse_begin.txt");
    a.read_to_string(&mut s).unwrap();
    assert_eq!(&s[..5], "test\n");
    assert!(s[5..].chars().all(|x| x == '\u{0}'));

    let mut a = entries.next().unwrap().unwrap();
    let mut s = String::new();
    assert_eq!(&*a.header().path_bytes(), b"sparse_end.txt");
    a.read_to_string(&mut s).unwrap();
    assert!(s[..s.len() - 9].chars().all(|x| x == '\u{0}'));
    assert_eq!(&s[s.len() - 9..], "test_end\n");

    let mut a = entries.next().unwrap().unwrap();
    let mut s = String::new();
    assert_eq!(&*a.header().path_bytes(), b"sparse_ext.txt");
    a.read_to_string(&mut s).unwrap();
    assert!(s[..0x1000].chars().all(|x| x == '\u{0}'));
    assert_eq!(&s[0x1000..0x1000 + 5], "text\n");
    assert!(s[0x1000 + 5..0x3000].chars().all(|x| x == '\u{0}'));
    assert_eq!(&s[0x3000..0x3000 + 5], "text\n");
    assert!(s[0x3000 + 5..0x5000].chars().all(|x| x == '\u{0}'));
    assert_eq!(&s[0x5000..0x5000 + 5], "text\n");
    assert!(s[0x5000 + 5..0x7000].chars().all(|x| x == '\u{0}'));
    assert_eq!(&s[0x7000..0x7000 + 5], "text\n");
    assert!(s[0x7000 + 5..0x9000].chars().all(|x| x == '\u{0}'));
    assert_eq!(&s[0x9000..0x9000 + 5], "text\n");
    assert!(s[0x9000 + 5..0xb000].chars().all(|x| x == '\u{0}'));
    assert_eq!(&s[0xb000..0xb000 + 5], "text\n");

    let mut a = entries.next().unwrap().unwrap();
    let mut s = String::new();
    assert_eq!(&*a.header().path_bytes(), b"sparse.txt");
    a.read_to_string(&mut s).unwrap();
    assert!(s[..0x1000].chars().all(|x| x == '\u{0}'));
    assert_eq!(&s[0x1000..0x1000 + 6], "hello\n");
    assert!(s[0x1000 + 6..0x2fa0].chars().all(|x| x == '\u{0}'));
    assert_eq!(&s[0x2fa0..0x2fa0 + 6], "world\n");
    assert!(s[0x2fa0 + 6..0x4000].chars().all(|x| x == '\u{0}'));

    assert!(entries.next().is_none());
}

#[test]
fn extract_sparse() {
    let rdr = random_cursor_reader(tar!("sparse.tar"));
    let mut ar = Archive::new(rdr);
    let td = TempBuilder::new().prefix("tar-rs").tempdir().unwrap();
    ar.unpack(td.path()).unwrap();

    let mut s = String::new();
    File::open(td.path().join("sparse_begin.txt"))
        .unwrap()
        .read_to_string(&mut s)
        .unwrap();
    assert_eq!(&s[..5], "test\n");
    assert!(s[5..].chars().all(|x| x == '\u{0}'));

    s.truncate(0);
    File::open(td.path().join("sparse_end.txt"))
        .unwrap()
        .read_to_string(&mut s)
        .unwrap();
    assert!(s[..s.len() - 9].chars().all(|x| x == '\u{0}'));
    assert_eq!(&s[s.len() - 9..], "test_end\n");

    s.truncate(0);
    File::open(td.path().join("sparse_ext.txt"))
        .unwrap()
        .read_to_string(&mut s)
        .unwrap();
    assert!(s[..0x1000].chars().all(|x| x == '\u{0}'));
    assert_eq!(&s[0x1000..0x1000 + 5], "text\n");
    assert!(s[0x1000 + 5..0x3000].chars().all(|x| x == '\u{0}'));
    assert_eq!(&s[0x3000..0x3000 + 5], "text\n");
    assert!(s[0x3000 + 5..0x5000].chars().all(|x| x == '\u{0}'));
    assert_eq!(&s[0x5000..0x5000 + 5], "text\n");
    assert!(s[0x5000 + 5..0x7000].chars().all(|x| x == '\u{0}'));
    assert_eq!(&s[0x7000..0x7000 + 5], "text\n");
    assert!(s[0x7000 + 5..0x9000].chars().all(|x| x == '\u{0}'));
    assert_eq!(&s[0x9000..0x9000 + 5], "text\n");
    assert!(s[0x9000 + 5..0xb000].chars().all(|x| x == '\u{0}'));
    assert_eq!(&s[0xb000..0xb000 + 5], "text\n");

    s.truncate(0);
    File::open(td.path().join("sparse.txt"))
        .unwrap()
        .read_to_string(&mut s)
        .unwrap();
    assert!(s[..0x1000].chars().all(|x| x == '\u{0}'));
    assert_eq!(&s[0x1000..0x1000 + 6], "hello\n");
    assert!(s[0x1000 + 6..0x2fa0].chars().all(|x| x == '\u{0}'));
    assert_eq!(&s[0x2fa0..0x2fa0 + 6], "world\n");
    assert!(s[0x2fa0 + 6..0x4000].chars().all(|x| x == '\u{0}'));
}

#[test]
fn large_sparse() {
    let rdr = random_cursor_reader(tar!("sparse-large.tar"));
    let mut ar = Archive::new(rdr);
    let mut entries = ar.entries().unwrap();
    // Only check the header info without extracting, as the file is very large,
    // and not all filesystems support sparse files.
    let a = entries.next().unwrap().unwrap();
    let h = a.header().as_gnu().unwrap();
    assert_eq!(h.real_size().unwrap(), 12626929280);
}

#[test]
fn sparse_with_trailing() {
    let rdr = random_cursor_reader(tar!("sparse-1.tar"));
    let mut ar = Archive::new(rdr);
    let mut entries = ar.entries().unwrap();
    let mut a = entries.next().unwrap().unwrap();
    let mut s = String::new();
    a.read_to_string(&mut s).unwrap();
    assert_eq!(0x100_00c, s.len());
    assert_eq!(&s[..0xc], "0MB through\n");
    assert!(s[0xc..0x100_000].chars().all(|x| x == '\u{0}'));
    assert_eq!(&s[0x100_000..], "1MB through\n");
}

#[test]
#[allow(clippy::option_map_unit_fn)]
fn writing_sparse() {
    let mut ar = Builder::new(Vec::new());
    let td = TempBuilder::new().prefix("tar-rs").tempdir().unwrap();

    let mut files = Vec::new();
    let mut append_file = |name: &str, chunks: &[(u64, u64)]| {
        let path = td.path().join(name);
        let mut file = File::create(&path).unwrap();
        file.set_len(
            chunks
                .iter()
                .map(|&(off, len)| off + len)
                .max()
                .unwrap_or(0),
        )
        .unwrap();
        for (i, &(off, len)) in chunks.iter().enumerate() {
            file.seek(io::SeekFrom::Start(off)).unwrap();
            let mut data = vec![i as u8 + b'a'; len as usize];
            data.first_mut().map(|x| *x = b'[');
            data.last_mut().map(|x| *x = b']');
            file.write_all(&data).unwrap();
        }
        ar.append_path_with_name(&path, path.file_name().unwrap())
            .unwrap();
        files.push(path);
    };

    append_file("empty", &[]);
    append_file("full_sparse", &[(0x20_000, 0)]);
    append_file("_x", &[(0x20_000, 0x1_000)]);
    append_file("x_", &[(0, 0x1_000), (0x20_000, 0)]);
    append_file("_x_x", &[(0x20_000, 0x1_000), (0x40_000, 0x1_000)]);
    append_file("x_x_", &[(0, 0x1_000), (0x20_000, 0x1_000), (0x40_000, 0)]);
    append_file("uneven", &[(0x20_333, 0x555), (0x40_777, 0x999)]);

    ar.finish().unwrap();

    let data = ar.into_inner().unwrap();

    // Without sparse support, the size of the tarball exceed 1MiB.
    #[cfg(target_os = "linux")]
    assert!(data.len() <= 37 * 1024); // ext4 (defaults to 4k block size)
    #[cfg(target_os = "freebsd")]
    assert!(data.len() <= 273 * 1024); // UFS (defaults to 32k block size, last block isn't a hole)

    let mut ar = Archive::new(&data[..]);
    let mut entries = ar.entries().unwrap();
    for path in files {
        let mut f = entries.next().unwrap().unwrap();

        let mut s = String::new();
        f.read_to_string(&mut s).unwrap();

        let expected = fs::read_to_string(&path).unwrap();

        assert!(s == expected, "path: {path:?}");
    }

    assert!(entries.next().is_none());
}

#[test]
fn path_separators() {
    let mut ar = Builder::new(Vec::new());
    let td = TempBuilder::new().prefix("tar-rs").tempdir().unwrap();

    let path = td.path().join("test");
    File::create(&path).unwrap().write_all(b"test").unwrap();

    let short_path: PathBuf = repeat("abcd").take(2).collect();
    let long_path: PathBuf = repeat("abcd").take(50).collect();

    // Make sure UStar headers normalize to Unix path separators
    let mut header = Header::new_ustar();

    header.set_path(&short_path).unwrap();
    assert_eq!(header.path().unwrap(), short_path);
    assert!(!header.path_bytes().contains(&b'\\'));

    header.set_path(&long_path).unwrap();
    assert_eq!(header.path().unwrap(), long_path);
    assert!(!header.path_bytes().contains(&b'\\'));

    // Make sure GNU headers normalize to Unix path separators,
    // including the `@LongLink` fallback used by `append_file`.
    ar.append_file(&short_path, &mut File::open(&path).unwrap())
        .unwrap();
    ar.append_file(&long_path, &mut File::open(&path).unwrap())
        .unwrap();

    let rd = Cursor::new(ar.into_inner().unwrap());
    let mut ar = Archive::new(rd);
    let mut entries = ar.entries().unwrap();

    let entry = entries.next().unwrap().unwrap();
    assert_eq!(entry.path().unwrap(), short_path);
    assert!(!entry.path_bytes().contains(&b'\\'));

    let entry = entries.next().unwrap().unwrap();
    assert_eq!(entry.path().unwrap(), long_path);
    assert!(!entry.path_bytes().contains(&b'\\'));

    assert!(entries.next().is_none());
}

#[test]
#[cfg(unix)]
fn append_path_symlink() {
    use std::borrow::Cow;
    use std::env;
    use std::os::unix::fs::symlink;

    let mut ar = Builder::new(Vec::new());
    ar.follow_symlinks(false);
    let td = TempBuilder::new().prefix("tar-rs").tempdir().unwrap();

    let long_linkname = "abcd".repeat(30);
    let long_pathname = "dcba".repeat(30);
    env::set_current_dir(td.path()).unwrap();
    // "short" path name / short link name
    symlink("testdest", "test").unwrap();
    ar.append_path("test").unwrap();
    // short path name / long link name
    symlink(&long_linkname, "test2").unwrap();
    ar.append_path("test2").unwrap();
    // long path name / long link name
    symlink(&long_linkname, &long_pathname).unwrap();
    ar.append_path(&long_pathname).unwrap();

    let rd = Cursor::new(ar.into_inner().unwrap());
    let mut ar = Archive::new(rd);
    let mut entries = ar.entries().unwrap();

    let entry = entries.next().unwrap().unwrap();
    assert_eq!(entry.path().unwrap(), Path::new("test"));
    assert_eq!(
        entry.link_name().unwrap(),
        Some(Cow::from(Path::new("testdest")))
    );
    assert_eq!(entry.header().size().unwrap(), 0);

    let entry = entries.next().unwrap().unwrap();
    assert_eq!(entry.path().unwrap(), Path::new("test2"));
    assert_eq!(
        entry.link_name().unwrap(),
        Some(Cow::from(Path::new(&long_linkname)))
    );
    assert_eq!(entry.header().size().unwrap(), 0);

    let entry = entries.next().unwrap().unwrap();
    assert_eq!(entry.path().unwrap(), Path::new(&long_pathname));
    assert_eq!(
        entry.link_name().unwrap(),
        Some(Cow::from(Path::new(&long_linkname)))
    );
    assert_eq!(entry.header().size().unwrap(), 0);

    assert!(entries.next().is_none());
}

#[test]
fn name_with_slash_doesnt_fool_long_link_and_bsd_compat() {
    let td = TempBuilder::new().prefix("tar-rs").tempdir().unwrap();

    let mut ar = Builder::new(Vec::new());

    let mut h = Header::new_gnu();
    h.set_path("././@LongLink").unwrap();
    h.set_size(4);
    h.set_entry_type(EntryType::new(b'L'));
    h.set_cksum();
    ar.append(&h, "foo\0".as_bytes()).unwrap();

    let mut header = Header::new_gnu();
    header.set_entry_type(EntryType::Regular);
    header.set_path("testdir/").unwrap();
    header.set_size(0);
    header.set_cksum();
    ar.append(&header, &mut io::empty()).unwrap();

    // Extracting
    let rdr = Cursor::new(ar.into_inner().unwrap());
    let mut ar = Archive::new(rdr);
    ar.unpack(td.path()).unwrap();

    // Iterating
    let rdr = Cursor::new(ar.into_inner().into_inner());
    let mut ar = Archive::new(rdr);
    assert!(ar.entries().unwrap().all(|fr| fr.is_ok()));

    assert!(td.path().join("foo").is_file());
}

#[test]
fn insert_local_file_different_name() {
    let mut ar = Builder::new(Vec::new());
    let td = TempBuilder::new().prefix("tar-rs").tempdir().unwrap();
    let path = td.path().join("directory");
    fs::create_dir(&path).unwrap();
    ar.append_path_with_name(&path, "archive/dir").unwrap();
    let path = td.path().join("file");
    File::create(&path).unwrap().write_all(b"test").unwrap();
    ar.append_path_with_name(&path, "archive/dir/f").unwrap();

    let rd = Cursor::new(ar.into_inner().unwrap());
    let mut ar = Archive::new(rd);
    let mut entries = ar.entries().unwrap();
    let entry = entries.next().unwrap().unwrap();
    assert_eq!(entry.path().unwrap(), Path::new("archive/dir"));
    let entry = entries.next().unwrap().unwrap();
    assert_eq!(entry.path().unwrap(), Path::new("archive/dir/f"));
    assert!(entries.next().is_none());
}

#[test]
#[cfg(unix)]
fn tar_directory_containing_symlink_to_directory() {
    use std::os::unix::fs::symlink;

    let td = TempBuilder::new().prefix("tar-rs").tempdir().unwrap();
    let dummy_src = TempBuilder::new().prefix("dummy_src").tempdir().unwrap();
    let dummy_dst = td.path().join("dummy_dst");
    let mut ar = Builder::new(Vec::new());
    symlink(dummy_src.path().display().to_string(), &dummy_dst).unwrap();

    assert!(dummy_dst.read_link().is_ok());
    assert!(dummy_dst.read_link().unwrap().is_dir());
    ar.append_dir_all("symlinks", td.path()).unwrap();
    ar.finish().unwrap();
}

#[test]
fn long_path() {
    let td = TempBuilder::new().prefix("tar-rs").tempdir().unwrap();
    let rdr = random_cursor_reader(tar!("7z_long_path.tar"));
    let mut ar = Archive::new(rdr);
    assert!(ar.unpack(td.path()).is_ok());
}

#[test]
fn unpack_path_larger_than_windows_max_path() {
    let dir_name = "iamaprettylongnameandtobepreciseiam91characterslongwhichsomethinkisreallylongandothersdonot";
    // 183 character directory name
    let really_long_path = format!("{}{}", dir_name, dir_name);
    let td = TempBuilder::new()
        .prefix(&really_long_path)
        .tempdir()
        .unwrap();
    // directory in 7z_long_path.tar is over 100 chars
    let rdr = random_cursor_reader(tar!("7z_long_path.tar"));
    let mut ar = Archive::new(rdr);
    // should unpack path greater than windows MAX_PATH length of 260 characters
    assert!(ar.unpack(td.path()).is_ok());
}

#[test]
fn append_long_multibyte() {
    let mut x = tar::Builder::new(Vec::new());
    let mut name = String::new();
    let data: &[u8] = &[];
    for _ in 0..512 {
        name.push('a');
        name.push('𑢮');
        x.append_data(&mut Header::new_gnu(), &name, data).unwrap();
        name.pop();
    }
}

#[test]
fn read_only_directory_containing_files() {
    let td = TempBuilder::new().prefix("tar-rs").tempdir().unwrap();

    let mut b = Builder::new(Vec::<u8>::new());

    let mut h = Header::new_gnu();
    h.set_path("dir/").unwrap();
    h.set_size(0);
    h.set_entry_type(EntryType::dir());
    h.set_mode(0o444);
    h.set_cksum();
    b.append(&h, "".as_bytes()).unwrap();

    let mut h = Header::new_gnu();
    h.set_path("dir/file").unwrap();
    h.set_size(2);
    h.set_entry_type(EntryType::file());
    h.set_cksum();
    b.append(&h, "hi".as_bytes()).unwrap();

    let contents = b.into_inner().unwrap();
    let mut ar = Archive::new(&contents[..]);
    assert!(ar.unpack(td.path()).is_ok());
}

// This test was marked linux only due to macOS CI can't handle `set_current_dir` correctly
#[test]
#[cfg(target_os = "linux")]
fn tar_directory_containing_special_files() {
    use std::env;
    use std::ffi::CString;

    let td = TempBuilder::new().prefix("tar-rs").tempdir().unwrap();
    let fifo = td.path().join("fifo");

    unsafe {
        let fifo_path = CString::new(fifo.to_str().unwrap()).unwrap();
        let ret = libc::mknod(fifo_path.as_ptr(), libc::S_IFIFO | 0o644, 0);
        if ret != 0 {
            libc::perror(fifo_path.as_ptr());
            panic!("Failed to create a FIFO file");
        }
    }

    env::set_current_dir(td.path()).unwrap();
    let mut ar = Builder::new(Vec::new());
    // append_path has a different logic for processing files, so we need to test it as well
    ar.append_path("fifo").unwrap();
    ar.append_dir_all("special", td.path()).unwrap();
    env::set_current_dir("/dev/").unwrap();
    // CI systems seem to have issues with creating a chr device
    ar.append_path("null").unwrap();
    ar.finish().unwrap();
}

#[test]
fn header_size_overflow() {
    // maximal file size doesn't overflow anything
    let mut ar = Builder::new(Vec::new());
    let mut header = Header::new_gnu();
    header.set_size(u64::MAX);
    header.set_cksum();
    ar.append(&header, "x".as_bytes()).unwrap();
    let result = ar.into_inner().unwrap();
    let mut ar = Archive::new(&result[..]);
    let mut e = ar.entries().unwrap();
    let err = e.next().unwrap().err().unwrap();
    assert!(
        err.to_string().contains("size overflow"),
        "bad error: {}",
        err
    );

    // back-to-back entries that would overflow also don't panic
    let mut ar = Builder::new(Vec::new());
    let mut header = Header::new_gnu();
    header.set_size(1_000);
    header.set_cksum();
    ar.append(&header, &[0u8; 1_000][..]).unwrap();
    let mut header = Header::new_gnu();
    header.set_size(u64::MAX - 513);
    header.set_cksum();
    ar.append(&header, "x".as_bytes()).unwrap();
    let result = ar.into_inner().unwrap();
    let mut ar = Archive::new(&result[..]);
    let mut e = ar.entries().unwrap();
    e.next().unwrap().unwrap();
    let err = e.next().unwrap().err().unwrap();
    assert!(
        err.to_string().contains("size overflow"),
        "bad error: {}",
        err
    );
}

#[test]
#[cfg(unix)]
fn ownership_preserving() {
    use std::os::unix::prelude::*;

    let mut rdr = Vec::new();
    let mut ar = Builder::new(&mut rdr);
    let data: &[u8] = &[];
    let mut header = Header::new_gnu();
    // file 1 with uid = 580800000, gid = 580800000
    header.set_gid(580800000);
    header.set_uid(580800000);
    header.set_path("iamuid580800000").unwrap();
    header.set_size(0);
    header.set_cksum();
    ar.append(&header, data).unwrap();
    // file 2 with uid = 580800001, gid = 580800000
    header.set_uid(580800001);
    header.set_path("iamuid580800001").unwrap();
    header.set_cksum();
    ar.append(&header, data).unwrap();
    // file 3 with uid = 580800002, gid = 580800002
    header.set_gid(580800002);
    header.set_uid(580800002);
    header.set_path("iamuid580800002").unwrap();
    header.set_cksum();
    ar.append(&header, data).unwrap();
    // directory 1 with uid = 580800002, gid = 580800002
    header.set_entry_type(EntryType::Directory);
    header.set_gid(580800002);
    header.set_uid(580800002);
    header.set_path("iamuid580800002dir").unwrap();
    header.set_cksum();
    ar.append(&header, data).unwrap();
    // symlink to file 1
    header.set_entry_type(EntryType::Symlink);
    header.set_gid(580800002);
    header.set_uid(580800002);
    header.set_path("iamuid580800000symlink").unwrap();
    header.set_cksum();
    ar.append_link(&mut header, "iamuid580800000symlink", "iamuid580800000")
        .unwrap();
    ar.finish().unwrap();

    let rdr = Cursor::new(ar.into_inner().unwrap());
    let td = TempBuilder::new().prefix("tar-rs").tempdir().unwrap();
    let mut ar = Archive::new(rdr);
    ar.set_preserve_ownerships(true);

    if unsafe { libc::getuid() } == 0 {
        ar.unpack(td.path()).unwrap();
        // validate against premade files
        // iamuid580800001 has this ownership: 580800001:580800000
        let meta = std::fs::symlink_metadata(td.path().join("iamuid580800000")).unwrap();
        assert_eq!(meta.uid(), 580800000);
        assert_eq!(meta.gid(), 580800000);
        let meta = std::fs::symlink_metadata(td.path().join("iamuid580800001")).unwrap();
        assert_eq!(meta.uid(), 580800001);
        assert_eq!(meta.gid(), 580800000);
        let meta = std::fs::symlink_metadata(td.path().join("iamuid580800002")).unwrap();
        assert_eq!(meta.uid(), 580800002);
        assert_eq!(meta.gid(), 580800002);
        let meta = std::fs::symlink_metadata(td.path().join("iamuid580800002dir")).unwrap();
        assert_eq!(meta.uid(), 580800002);
        assert_eq!(meta.gid(), 580800002);
        let meta = std::fs::symlink_metadata(td.path().join("iamuid580800000symlink")).unwrap();
        assert_eq!(meta.uid(), 580800002);
        assert_eq!(meta.gid(), 580800002)
    } else {
        // it's not possible to unpack tar while preserving ownership
        // without root permissions
        assert!(ar.unpack(td.path()).is_err());
    }
}

#[test]
#[cfg(unix)]
fn pax_and_gnu_uid_gid() {
    let tarlist = [tar!("biguid_gnu.tar"), tar!("biguid_pax.tar")];

    for file in &tarlist {
        let td = TempBuilder::new().prefix("tar-rs").tempdir().unwrap();
        let rdr = random_cursor_reader(file);
        let mut ar = Archive::new(rdr);
        ar.set_preserve_ownerships(true);

        if unsafe { libc::getuid() } == 0 {
            ar.unpack(td.path()).unwrap();
            let meta = fs::metadata(td.path().join("test.txt")).unwrap();
            let uid = std::os::unix::prelude::MetadataExt::uid(&meta);
            let gid = std::os::unix::prelude::MetadataExt::gid(&meta);
            // 4294967294 = u32::MAX - 1
            assert_eq!(uid, 4294967294);
            assert_eq!(gid, 4294967294);
        } else {
            // it's not possible to unpack tar while preserving ownership
            // without root permissions
            assert!(ar.unpack(td.path()).is_err());
        }
    }
}

#[test]
fn append_data_error_does_not_corrupt_subsequent_entries() {
    // When append_data fails (e.g., path contains ".."), subsequent
    // successful writes must not be corrupted by an orphaned GNU
    // long-name extension entry left in the stream.
    let mut ar = Builder::new(Vec::new());

    // First write: a long path (>100 bytes to trigger GNU long-name extension)
    // containing ".." not as the last component, which will fail validation
    // in set_truncated_path_for_gnu_header.
    let dotdot_path = "a/../b/".to_string() + &"c".repeat(100);
    let mut header = Header::new_gnu();
    header.set_size(5);
    header.set_cksum();
    let result = ar.append_data(&mut header, &dotdot_path, &b"first"[..]);
    assert!(result.is_err());

    // Second write: a clean path that should succeed normally.
    let mut header = Header::new_gnu();
    header.set_size(6);
    header.set_cksum();
    ar.append_data(&mut header, "clean.txt", &b"second"[..])
        .unwrap();

    // Verify: the archive should contain exactly one entry at "clean.txt"
    // with content "second". Before the fix, it contained an entry at the
    // dotdot path with content "second" — the orphaned long-name stole the data.
    let data = ar.into_inner().unwrap();
    let mut archive = Archive::new(&data[..]);
    let entries: Vec<_> = archive
        .entries()
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].path().unwrap().to_str().unwrap(), "clean.txt");
}

/// Build the PAX size smuggling archive described in the original report.
///
/// A PAX extended header declares `size=2048` for a regular file whose
/// actual header size field is 8. A symlink entry is hidden inside the
/// inflated region. A correct parser honours the PAX size and skips over
/// the symlink; a buggy one reads only the header size and exposes it.
fn build_pax_smuggle_archive() -> Vec<u8> {
    const B: usize = 512;
    const INFLATED: usize = 2048;
    let end_of_archive = || std::iter::repeat(0u8).take(B * 2);

    let mut ar: Vec<u8> = Vec::new();

    // PAX extended header declaring size=2048 for the next entry.
    let pax_rec = format!("13 size={INFLATED}\n");
    let mut pax_hdr = Header::new_ustar();
    pax_hdr.set_path("./PaxHeaders/regular").unwrap();
    pax_hdr.set_size(pax_rec.as_bytes().len() as u64);
    pax_hdr.set_entry_type(EntryType::XHeader);
    pax_hdr.set_cksum();
    ar.extend_from_slice(pax_hdr.as_bytes());
    ar.extend_from_slice(pax_rec.as_bytes());
    ar.resize(ar.len().next_multiple_of(B), 0);

    // Regular file whose header says size=8, but PAX says 2048.
    let content = b"regular\n";
    let mut file_hdr = Header::new_ustar();
    file_hdr.set_path("regular.txt").unwrap();
    file_hdr.set_size(content.len() as u64);
    file_hdr.set_entry_type(EntryType::Regular);
    file_hdr.set_cksum();
    ar.extend_from_slice(file_hdr.as_bytes());
    let mark = ar.len();
    ar.extend_from_slice(content);
    ar.resize(ar.len().next_multiple_of(B), 0);

    // Smuggled symlink hidden in the inflated region.
    let mut sym_hdr = Header::new_ustar();
    sym_hdr.set_path("smuggled").unwrap();
    sym_hdr.set_size(0);
    sym_hdr.set_entry_type(EntryType::Symlink);
    sym_hdr.set_link_name("/etc/shadow").unwrap();
    sym_hdr.set_cksum();
    ar.extend_from_slice(sym_hdr.as_bytes());
    ar.extend(end_of_archive());

    // Pad to fill the inflated window.
    let used = ar.len() - mark;
    let pad = INFLATED.saturating_sub(used);
    ar.extend(std::iter::repeat(0u8).take(pad.next_multiple_of(B)));

    // End-of-archive.
    ar.extend(end_of_archive());
    ar
}

/// Regression test for PAX size smuggling.
///
/// A crafted archive uses a PAX extended header to declare a file size (2048)
/// larger than the header's octal size field (8). Before the fix, `tar-rs`
/// only applied the PAX size override when the header size was 0, so it would
/// read the small header size, advance too little, and expose a symlink entry
/// hidden in the "padding" area. After the fix, the PAX size unconditionally
/// overrides the header size, causing the parser to skip over the smuggled
/// symlink — matching the behavior of compliant parsers.
#[test]
fn pax_size_smuggled_symlink() {
    let data = build_pax_smuggle_archive();

    let mut archive = Archive::new(random_cursor_reader(&data[..]));
    let entries: Vec<_> = archive
        .entries()
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            let path = e.path().unwrap().to_path_buf();
            let kind = e.header().entry_type();
            let link = e.link_name().unwrap().map(|l| l.to_path_buf());
            (path, kind, link)
        })
        .collect();

    // With the fix applied, only "regular.txt" should be visible.
    // The smuggled symlink must NOT appear.
    let expected: Vec<(PathBuf, EntryType, Option<PathBuf>)> =
        vec![(PathBuf::from("regular.txt"), EntryType::Regular, None)];
    assert_eq!(
        entries, expected,
        "smuggled symlink visible or unexpected entries\n\
         got: {entries:?}"
    );
}

/// Cross-validate that `tar` and `astral-tokio-tar` parse the PAX size
/// smuggling archive identically, guarding against parsing differentials.
#[tokio::test]
async fn pax_size_smuggle_matches_astral_tokio_tar() {
    use tokio_stream::StreamExt;

    let data = build_pax_smuggle_archive();

    // Parse with sync tar.
    let sync_entries: Vec<_> = {
        let mut ar = Archive::new(&data[..]);
        ar.entries()
            .unwrap()
            .map(|e| {
                let e = e.unwrap();
                let path = e.path().unwrap().to_path_buf();
                let kind = e.header().entry_type();
                let link = e.link_name().unwrap().map(|l| l.to_path_buf());
                (path, kind, link)
            })
            .collect()
    };

    // Parse with async astral-tokio-tar.
    let async_entries: Vec<_> = {
        let mut ar = tokio_tar::Archive::new(&data[..]);
        let mut entries = ar.entries().unwrap();
        let mut result = Vec::new();
        while let Some(e) = entries.next().await {
            let e = e.unwrap();
            let entry_type = e.header().entry_type();
            result.push((
                e.path().unwrap().to_path_buf(),
                // Map through the raw byte so the two crates' EntryTypes compare.
                EntryType::new(entry_type.as_byte()),
                e.link_name().unwrap().map(|l| l.to_path_buf()),
            ));
        }
        result
    };

    // Assert exact expected content for both parsers independently,
    // so we verify correctness — not just mutual agreement.
    let expected: Vec<(PathBuf, EntryType, Option<PathBuf>)> =
        vec![(PathBuf::from("regular.txt"), EntryType::Regular, None)];

    assert_eq!(
        sync_entries, expected,
        "tar-rs produced unexpected entries (smuggled symlink visible?)\n\
         got: {sync_entries:?}"
    );
    assert_eq!(
        async_entries, expected,
        "astral-tokio-tar produced unexpected entries (smuggled symlink visible?)\n\
         got: {async_entries:?}"
    );
}

#[test]
fn pax_size_does_not_apply_to_extension_headers() {
    // This archive is ordered as `x (PAX, size=2048) → L (GNU longname) → file_a → file_b`.
    // If the PAX `size=` is wrongly applied to the intermediary `L` header, the
    // parser advances the cursor by 2048 bytes after `L` instead of by `L`'s true
    // payload size, landing in the middle of `file_a`'s body and causing a
    // checksum error that hides both `file_a` and `file_b`. This is Scenario A of
    // GHSA-3cv2-h65g-fgmm. Correct parsing applies PAX only to the next *file*
    // entry, so the L longname renames `file_a` to `longname.txt` and the stream
    // yields ["longname.txt", "file_b"]. Mirrors astral-tokio-tar commit 36e734d.
    let bytes = tar!("pax-overrides-extension-header.tar");
    let mut ar = Archive::new(Cursor::new(bytes));
    let entries: Vec<String> = ar
        .entries()
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            e.path().unwrap().to_str().unwrap().to_owned()
        })
        .collect();
    assert_eq!(entries, vec!["longname.txt", "file_b"]);
}

/// Cross-validate that `tar` and `astral-tokio-tar` (>= 0.6.2) parse the
/// GHSA-3cv2-h65g-fgmm test archive identically, guarding against any future
/// re-introduction of a parsing differential between the two crates.
#[tokio::test]
async fn pax_extension_header_matches_astral_tokio_tar() {
    use tokio_stream::StreamExt;

    let bytes = tar!("pax-overrides-extension-header.tar");

    // Parse with sync tar-rs.
    let sync_entries: Vec<String> = {
        let mut ar = Archive::new(Cursor::new(bytes));
        ar.entries()
            .unwrap()
            .map(|e| {
                let e = e.unwrap();
                e.path().unwrap().to_str().unwrap().to_owned()
            })
            .collect()
    };

    // Parse with async astral-tokio-tar (>= 0.6.2, which carries the fix).
    let async_entries: Vec<String> = {
        let mut ar = tokio_tar::Archive::new(bytes);
        let mut entries = ar.entries().unwrap();
        let mut result = Vec::new();
        while let Some(e) = entries.next().await {
            let e = e.unwrap();
            result.push(e.path().unwrap().to_str().unwrap().to_owned());
        }
        result
    };

    let expected = vec!["longname.txt".to_owned(), "file_b".to_owned()];

    assert_eq!(
        sync_entries, expected,
        "tar-rs produced unexpected entries\ngot: {sync_entries:?}"
    );
    assert_eq!(
        async_entries, expected,
        "astral-tokio-tar produced unexpected entries\ngot: {async_entries:?}"
    );
}
