use super::{OsStr, OsStrExt, OsString, OsStringExt, PathBuf, join_paths, split_paths};

#[test]
fn preserve_empty_entries_quotes_and_non_utf8() {
    let value = OsString::from_vec(b":/bin:\xff:\"quoted\":".to_vec());
    let paths: Vec<_> = split_paths(&value).collect();
    assert_eq!(paths.len(), 5);
    assert_eq!(paths[0], PathBuf::new());
    assert_eq!(paths[2].as_os_str().as_bytes(), b"\xff");
    assert_eq!(paths[3], PathBuf::from(r#""quoted""#));
    assert_eq!(join_paths(&paths).unwrap(), value);
}

#[test]
fn reject_ambiguous_path_entry() {
    assert!(join_paths(["/safe", "/contains:separator"]).is_err());
}

#[test]
fn empty_path_has_one_empty_entry() {
    assert_eq!(split_paths(OsStr::new("")).collect::<Vec<_>>(), vec![PathBuf::new()]);
    assert_eq!(join_paths(Vec::<PathBuf>::new()).unwrap(), OsString::new());
}
