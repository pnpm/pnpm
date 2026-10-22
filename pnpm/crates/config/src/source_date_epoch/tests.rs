use super::{EnvVar, read_source_date_epoch};
use std::{
    cell::RefCell,
    time::{Duration, SystemTime},
};

thread_local! {
    static VALUE: RefCell<Option<String>> = const { RefCell::new(None) };
}

struct FakeEnv;

impl EnvVar for FakeEnv {
    fn var(name: &str) -> Option<String> {
        assert_eq!(name, "SOURCE_DATE_EPOCH");
        VALUE.with_borrow(Clone::clone)
    }
}

fn read_with(value: Option<&str>) -> Option<SystemTime> {
    VALUE.set(value.map(str::to_string));
    read_source_date_epoch::<FakeEnv>()
}

#[test]
fn whole_seconds_set_the_time() {
    assert_eq!(
        read_with(Some("1700000000")),
        Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000)),
    );
    assert_eq!(read_with(Some("0")), Some(SystemTime::UNIX_EPOCH));
}

#[test]
fn unset_or_malformed_values_are_ignored() {
    assert_eq!(read_with(None), None);
    assert_eq!(read_with(Some("")), None);
    assert_eq!(read_with(Some("-1")), None);
    assert_eq!(read_with(Some("1700000000.5")), None);
    assert_eq!(read_with(Some("yesterday")), None);
    assert_eq!(read_with(Some(&(i64::MAX as u64 / 1_000 + 1).to_string())), None);
}
