use std::cell::Cell;

use pnpr_config::Config;
use pnpr_fixtures::MockConfigPresets;
use pnpr_policy::{
    AccessList,
    Identity,
    PackageRule,
    PackageRules,
};
use pnpr_registry::{
    Ecosystem,
    PackagePattern,
};
use pnpr_route::{
    Footprint,
    PrivateAccessDescriptor,
    RouteContext,
    headers_credential_digest,
};
use reqwest::header::{
    AUTHORIZATION,
    HeaderMap,
    HeaderValue,
};
use tempfile::TempDir;

use super::{
    Map,
    Value,
    VerdictCache,
    VerdictLookup,
    VerdictRecord,
};

const SECRET: &[u8] = b"0123456789abcdef0123456789abcdef";

fn open() -> (TempDir, VerdictCache) {
    let dir = TempDir::new().expect("tempdir");
    let cache = VerdictCache::open(&dir.path().join("verdicts.sqlite")).expect("open cache");
    (dir, cache)
}

fn policy(min_age: i64) -> Map<String, Value> {
    let mut map = Map::new();
    map.insert("minimumReleaseAge".to_string(), Value::from(min_age));
    map
}

fn default_context() -> RouteContext {
    let config = Config::proxy("127.0.0.1:7677".parse().unwrap(), "/tmp/pnpr-verdict".into());
    RouteContext::from_config(&config)
}

fn lookup<'a>(
    hash: &'a str,
    epoch: &'a str,
    route_context: &'a RouteContext,
    identity: &'a Identity,
) -> VerdictLookup<'a> {
    VerdictLookup { hash, epoch, secret: SECRET, route_context, identity }
}

fn record_public(cache: &VerdictCache, hash: &str, epoch: &str, min_age: i64) {
    let footprint = Footprint::default();
    cache.record(VerdictRecord {
        hash,
        footprint: &footprint,
        secret: SECRET,
        epoch,
        policy: policy(min_age),
    });
}

#[test]
fn recorded_pass_is_a_hit_when_policy_trusted() {
    let (_dir, cache) = open();
    let context = default_context();
    record_public(&cache, "hash-a", "1:1", 1440);
    assert_eq!(
        cache.verified_footprint(&lookup("hash-a", "1:1", &context, &Identity::Anonymous), |_| {
            true
        }),
        Some(Footprint::default()),
    );
}

#[test]
fn absent_hash_is_a_miss() {
    let (_dir, cache) = open();
    let context = default_context();
    assert_eq!(
        cache.verified_footprint(
            &lookup("never-recorded", "1:1", &context, &Identity::Anonymous),
            |_| true,
        ),
        None,
    );
}

#[test]
fn untrusted_cached_policy_is_a_miss() {
    let (_dir, cache) = open();
    let context = default_context();
    record_public(&cache, "hash-a", "1:1", 1440);
    assert_eq!(
        cache.verified_footprint(&lookup("hash-a", "1:1", &context, &Identity::Anonymous), |_| {
            false
        }),
        None,
    );
}

#[test]
fn hit_hands_the_cached_policy_to_the_trust_check() {
    let (_dir, cache) = open();
    let context = default_context();
    record_public(&cache, "hash-a", "1:1", 1440);
    let saw_policy = Cell::new(false);
    let hit = cache.verified_footprint(
        &lookup("hash-a", "1:1", &context, &Identity::Anonymous),
        |cached| {
            saw_policy.set(cached.get("minimumReleaseAge") == Some(&Value::from(1440)));
            true
        },
    );
    assert!(hit.is_some());
    assert!(saw_policy.get(), "the stored policy snapshot is what the trust check receives");
}

#[test]
fn corrupt_rows_are_evicted_and_epoch_mismatch_preserves_other_epochs() {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("verdicts.sqlite");
    let cache = VerdictCache::open(&path).expect("open cache");
    let context = default_context();

    {
        let conn = rusqlite::Connection::open(&path).expect("open raw conn");
        conn.execute(
            "INSERT INTO lockfile_verdicts (
                 hash, footprint_key, footprint, policy, epoch, verified_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params!["corrupt-policy", "", "[]", "not json", "1:1", 0_i64],
        )
        .expect("insert corrupt policy");
        conn.execute(
            "INSERT INTO lockfile_verdicts (
                 hash, footprint_key, footprint, policy, epoch, verified_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params!["corrupt-footprint", "", "not json", "{}", "1:1", 0_i64],
        )
        .expect("insert corrupt footprint");
        conn.execute(
            "INSERT INTO lockfile_verdicts (
                 hash, footprint_key, footprint, policy, epoch, verified_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params!["tampered-key", "wrong-hmac", "[]", "{}", "1:1", 0_i64],
        )
        .expect("insert tampered key");
    }

    for hash in ["corrupt-policy", "corrupt-footprint", "tampered-key"] {
        assert_eq!(
            cache.verified_footprint(&lookup(hash, "1:1", &context, &Identity::Anonymous), |_| {
                true
            }),
            None,
            "{hash} must miss",
        );
    }

    let conn = rusqlite::Connection::open(&path).expect("open raw conn");
    let remaining: i64 = conn
        .query_row("SELECT COUNT(*) FROM lockfile_verdicts", [], |row| row.get(0))
        .expect("count rows");
    assert_eq!(remaining, 0, "corrupt and tampered rows self-heal on lookup");

    record_public(&cache, "epoch-isolated", "1:2", 1440);
    assert_eq!(
        cache.verified_footprint(
            &lookup("epoch-isolated", "1:1", &context, &Identity::Anonymous),
            |_| true,
        ),
        None,
    );
    record_public(&cache, "epoch-isolated", "1:1", 60);
    let current_hit = cache.verified_footprint(
        &lookup("epoch-isolated", "1:2", &context, &Identity::Anonymous),
        |cached| cached.get("minimumReleaseAge") == Some(&Value::from(1440)),
    );
    assert!(current_hit.is_some(), "an older-epoch lookup or write preserves the newer-epoch pass");
}

#[test]
fn re_recording_the_same_hash_and_footprint_overwrites_the_policy() {
    let (_dir, cache) = open();
    let context = default_context();
    record_public(&cache, "hash-a", "1:1", 1440);
    record_public(&cache, "hash-a", "1:1", 60);
    let hit = cache.verified_footprint(
        &lookup("hash-a", "1:1", &context, &Identity::Anonymous),
        |cached| {
            assert_eq!(cached.get("minimumReleaseAge"), Some(&Value::from(60)));
            true
        },
    );
    assert!(hit.is_some());
}

#[test]
fn private_hosted_verdict_requires_caller_footprint_authorization_without_evicting_row() {
    let (_dir, cache) = open();
    let mut config = Config::proxy("127.0.0.1:7677".parse().unwrap(), "/tmp/pnpr-verdict".into());
    config.routing.hosted.get_mut("local").unwrap().rules = PackageRules::new(
        vec![PackageRule {
            pattern: PackagePattern::parse("@acme/*", Ecosystem::Npm).unwrap(),
            access: Some(AccessList::from_tokens(["alice"])),
            publish: None,
            unpublish: None,
        }],
        Some(AccessList::from_tokens(["alice"])),
    );
    let context = RouteContext::from_config(&config);

    let mut private_footprint = Footprint::default();
    private_footprint.add(PrivateAccessDescriptor::Hosted {
        policy_id: "local\0@acme/born-priv".to_string(),
    });
    cache.record(VerdictRecord {
        hash: "priv-hash",
        footprint: &private_footprint,
        secret: SECRET,
        epoch: "1:1",
        policy: policy(60),
    });

    let alice = Identity::user("alice");
    let bob = Identity::user("bob");
    assert_eq!(
        cache.verified_footprint(&lookup("priv-hash", "1:1", &context, &bob), |_| true),
        None,
    );
    assert_eq!(
        cache.verified_footprint(
            &lookup("priv-hash", "1:1", &context, &Identity::Anonymous),
            |_| { true }
        ),
        None,
    );
    assert_eq!(
        cache.verified_footprint(&lookup("priv-hash", "1:1", &context, &alice), |_| true),
        Some(private_footprint),
        "unauthorized lookups must not evict alice's valid private verdict",
    );

    cache.clear();
    assert_eq!(
        cache.verified_footprint(&lookup("priv-hash", "1:1", &context, &alice), |_| true),
        None,
        "clear() drops all cached verdicts",
    );
}

#[test]
fn distinct_private_alias_footprints_coexist_for_the_same_lockfile_hash() {
    let (_dir, cache) = open();
    let mut config = Config::proxy("127.0.0.1:7677".parse().unwrap(), "/tmp/pnpr-verdict".into());
    for (alias, token, subject) in
        [("corp-a", "Bearer secret-a", "alice"), ("corp-b", "Bearer secret-b", "carol")]
    {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static(token));
        let cred = headers_credential_digest(&headers);
        let mut upstream = pnpr_config::UpstreamConfig::with_defaults(
            "https://npm.corp.example/".to_string(),
            headers,
        );
        upstream.set_access(AccessList::from_tokens([subject]));
        config.routing.upstreams.insert(alias.to_string(), upstream);

        let mut footprint = Footprint::default();
        footprint.add(PrivateAccessDescriptor::Alias {
            alias: alias.to_string(),
            credential_digest: cred,
            package: None,
        });
        cache.record(VerdictRecord {
            hash: "shared-hash",
            footprint: &footprint,
            secret: SECRET,
            epoch: "1:1",
            policy: policy(60),
        });
    }
    let context = RouteContext::from_config(&config);

    assert!(
        cache
            .verified_footprint(
                &lookup("shared-hash", "1:1", &context, &Identity::user("alice")),
                |_| true
            )
            .is_some(),
    );
    assert!(
        cache
            .verified_footprint(
                &lookup("shared-hash", "1:1", &context, &Identity::user("carol")),
                |_| true
            )
            .is_some(),
    );
    assert!(
        cache
            .verified_footprint(
                &lookup("shared-hash", "1:1", &context, &Identity::user("bob")),
                |_| true
            )
            .is_none(),
    );
}
