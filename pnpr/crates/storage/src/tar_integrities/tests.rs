use super::{CanonicalPackageName, TarIntegrityCache};
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};

#[test]
fn a_read_that_started_before_a_record_keeps_the_recorded_table() {
    let cache = TarIntegrityCache::default();
    let name = CanonicalPackageName::parse("left-pad", pnpr_package_name::Ecosystem::Npm).unwrap();
    let read_started = Instant::now();
    let recorded = Arc::new(BTreeMap::from([("tgz".to_string(), "tar".to_string())]));
    cache.put(&name, Arc::clone(&recorded), read_started + Duration::from_millis(1));
    cache.put(&name, Arc::default(), read_started);
    assert_eq!(cache.get(&name), Some(recorded));
}
