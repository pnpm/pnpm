use super::{
    Authority, CacheKey, Conflict, ConflictMonitor, LogEntry, LogEntryError, RecordIdentity,
    RecordKey,
};
use crate::{digest::RecordDigest, profile::Profile, purl::Purl};
use pretty_assertions::{assert_eq, assert_ne};

fn npm_key(revision: u64) -> RecordKey {
    RecordKey {
        identity: RecordIdentity::Package(Purl::npm("@example/widgets", "1.2.0").unwrap()),
        revision: Some(revision),
        profile: Profile::NPM_INSTALL_MANIFEST.parse().unwrap(),
    }
}

fn oci_key(revision: u64) -> RecordKey {
    RecordKey {
        identity: RecordIdentity::OciTag {
            repository: "acme/app".to_owned(),
            tag: "1.0".to_owned(),
        },
        revision: Some(revision),
        profile: Profile::OCI_TAG.parse().unwrap(),
    }
}

#[test]
fn names_statement_subjects() {
    assert_eq!(npm_key(0).subject_name(), "pkg:npm/%40example/widgets@1.2.0");
    assert_eq!(oci_key(3).subject_name(), "acme/app:1.0+r3");
}

#[test]
fn formats_and_parses_log_entries() {
    let digest = RecordDigest::of(b"record");
    let entry = LogEntry::new(npm_key(0), digest).unwrap();
    let expected = format!(
        "pnpr-log-entry/1\npurl pkg:npm/%40example/widgets@1.2.0\nrevision 0\n\
         profile npm-install-manifest/1\ndigest {digest}\n",
    );
    assert_eq!(String::from_utf8(entry.to_bytes()).unwrap(), expected);
    assert_eq!(LogEntry::parse(&entry.to_bytes()).unwrap(), entry);

    let oci = LogEntry::new(oci_key(4), digest).unwrap();
    let text = String::from_utf8(oci.to_bytes()).unwrap();
    assert!(
        text.starts_with("pnpr-log-entry/1\nrepository acme/app\ntag 1.0\nrevision 4\n"),
        "{text}",
    );
    assert_eq!(LogEntry::parse(&oci.to_bytes()).unwrap(), oci);

    let cargo_key = RecordKey {
        identity: RecordIdentity::Package(Purl::cargo("serde", "1.0.210").unwrap()),
        revision: None,
        profile: Profile::CARGO_INDEX_ENTRY.parse().unwrap(),
    };
    let cargo = LogEntry::new(cargo_key, digest).unwrap();
    assert!(!String::from_utf8(cargo.to_bytes()).unwrap().contains("revision"));
    assert_eq!(LogEntry::parse(&cargo.to_bytes()).unwrap(), cargo);
}

#[test]
fn rejects_non_canonical_log_entries() {
    let entry = LogEntry::new(npm_key(7), RecordDigest::of(b"record")).unwrap();
    let text = String::from_utf8(entry.to_bytes()).unwrap();
    for mangled in [
        text.replace("revision 7", "revision 07"),
        text.replace("pnpr-log-entry/1", "pnpr-log-entry/2"),
        text.trim_end().to_owned(),
        format!("{text}extra 1\n"),
        text.replace('\n', "\r\n"),
        text.replace("profile", "profile "),
        text.replace(
            "revision 7\nprofile npm-install-manifest/1",
            "profile npm-install-manifest/1\nrevision 7",
        ),
    ] {
        assert_eq!(
            LogEntry::parse(mangled.as_bytes()),
            Err(LogEntryError::Malformed),
            "{mangled:?}",
        );
    }
}

#[test]
fn rejects_log_entries_that_are_not_utf8() {
    let entry = LogEntry::new(npm_key(7), RecordDigest::of(b"record")).unwrap();
    let mut bytes = entry.to_bytes();
    let at = bytes
        .iter()
        .position(|&byte| byte == b'@')
        .unwrap();
    bytes[at] = 0xff;
    assert_eq!(LogEntry::parse(&bytes), Err(LogEntryError::Malformed));
}

#[test]
fn rejects_entries_that_would_not_parse_back() {
    let mut key = oci_key(1);
    key.identity = RecordIdentity::OciTag {
        repository: "acme/app".to_owned(),
        tag: "1.0\nrevision 9".to_owned(),
    };
    assert_eq!(
        LogEntry::new(key, RecordDigest::of(b"")),
        Err(LogEntryError::InvalidField { field: "tag" }),
    );
    let mut key = oci_key(1);
    key.revision = None;
    let oci = Profile::OCI_TAG.parse().unwrap();
    assert_eq!(
        LogEntry::new(key, RecordDigest::of(b"")),
        Err(LogEntryError::MissingRevision { profile: oci }),
    );
}

#[test]
fn log_entries_describe_the_identity_their_profile_names() {
    let mut oci_as_crate = oci_key(1);
    oci_as_crate.profile = Profile::CARGO_INDEX_ENTRY.parse().unwrap();
    let mut npm_as_oci = npm_key(0);
    npm_as_oci.profile = Profile::OCI_TAG.parse().unwrap();
    let mut npm_as_crate = npm_key(0);
    npm_as_crate.profile = Profile::CARGO_INDEX_ENTRY.parse().unwrap();
    npm_as_crate.revision = None;
    for key in [oci_as_crate, npm_as_oci, npm_as_crate] {
        let error = LogEntry::new(key.clone(), RecordDigest::of(b"")).unwrap_err();
        assert!(matches!(error, LogEntryError::ProfileMismatch { .. }), "{key:?}: {error}");
    }
    let mut future_oci = oci_key(1);
    future_oci.profile = "oci-tag/2".parse().unwrap();
    LogEntry::new(future_oci, RecordDigest::of(b"")).unwrap();
}

#[test]
fn log_entries_name_a_package_by_its_canonical_purl() {
    let mut key = npm_key(0);
    key.identity = RecordIdentity::Package(Purl::from_string_unchecked(
        "pkg:npm/@example/widgets@1.2.0".to_owned(),
    ));
    let error = LogEntry::new(key, RecordDigest::of(b"")).unwrap_err();
    assert!(matches!(error, LogEntryError::Purl(_)), "{error}");
}

#[test]
fn log_entries_carry_a_revision_exactly_when_their_profile_has_revisions() {
    let mut npm = npm_key(0);
    npm.revision = None;
    assert_eq!(
        LogEntry::new(npm, RecordDigest::of(b"")),
        Err(LogEntryError::MissingRevision {
            profile: Profile::NPM_INSTALL_MANIFEST.parse().unwrap()
        }),
    );
    let cargo = RecordKey {
        identity: RecordIdentity::Package(Purl::cargo("serde", "1.0.210").unwrap()),
        revision: Some(0),
        profile: Profile::CARGO_INDEX_ENTRY.parse().unwrap(),
    };
    assert_eq!(
        LogEntry::new(cargo.clone(), RecordDigest::of(b"")),
        Err(LogEntryError::UnexpectedRevision { profile: cargo.profile.clone() }),
    );
    LogEntry::new(RecordKey { revision: None, ..cargo }, RecordDigest::of(b"")).unwrap();
    let mut future = npm_key(0);
    future.profile = "npm-install-manifest/2".parse().unwrap();
    for revision in [None, Some(0)] {
        let key = RecordKey { revision, ..future.clone() };
        LogEntry::new(key, RecordDigest::of(b"")).unwrap();
    }
}

#[test]
fn a_monitor_reports_a_record_logged_with_two_digests() {
    let mut monitor = ConflictMonitor::default();
    let first = LogEntry::new(npm_key(0), RecordDigest::of(b"genuine")).unwrap();
    let repeat = first.clone();
    let next_revision = LogEntry::new(npm_key(1), RecordDigest::of(b"patched")).unwrap();
    let forged = LogEntry::new(npm_key(0), RecordDigest::of(b"forged")).unwrap();
    assert_eq!(monitor.observe(0, &first), None);
    assert_eq!(monitor.observe(1, &repeat), None);
    assert_eq!(monitor.observe(2, &next_revision), None);
    assert_eq!(
        monitor.observe(3, &forged),
        Some(Conflict {
            key: npm_key(0),
            first: (0, RecordDigest::of(b"genuine")),
            second: (3, RecordDigest::of(b"forged")),
        }),
    );
}

#[test]
fn formats_and_parses_authorities() {
    let authority = Authority::of_first_root("registry.example/npm", b"first root");
    let text = authority.to_string();
    assert!(text.starts_with("registry.example/npm+sha256:"), "{text}");
    assert_eq!(text.len(), "registry.example/npm+sha256:".len() + 64);
    assert_eq!(text.parse::<Authority>().unwrap(), authority);
    for bad in [
        "registry.example+sha256:00",
        "+sha256:0000000000000000000000000000000000000000000000000000000000000000",
        "a b+sha256:0000000000000000000000000000000000000000000000000000000000000000",
        "registry.example+sha256:000000000000000000000000000000000000000000000000000000000000000A",
        "registry.example+sha512:0000000000000000000000000000000000000000000000000000000000000000",
    ] {
        assert!(bad.parse::<Authority>().is_err(), "accepted {bad}");
    }
}

#[test]
fn cache_keys_separate_authorities_profiles_and_principals() {
    let first = Authority::of_first_root("registry.example/npm", b"root a");
    let second = Authority::of_first_root("registry.example/npm", b"root b");
    let key = |authority: &Authority, record: RecordKey, principal: Option<&str>| {
        CacheKey { authority: authority.clone(), record, principal: principal.map(str::to_owned) }
            .file_name()
    };
    let base = key(&first, npm_key(0), None);
    assert_eq!(base.len(), 64);
    assert_eq!(base, key(&first, npm_key(0), None));
    assert_ne!(base, key(&second, npm_key(0), None));
    assert_ne!(base, key(&first, npm_key(1), None));
    assert_ne!(base, key(&first, npm_key(0), Some("token:ci-publisher")));
    assert_ne!(base, key(&first, npm_key(0), Some("-")));
    assert_ne!(base, key(&first, npm_key(0), Some("")));
    let mut other_profile = npm_key(0);
    other_profile.profile = "npm-install-manifest/2".parse().unwrap();
    assert_ne!(base, key(&first, other_profile, None));
}
