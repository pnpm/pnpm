//! Access rules written inside a test's registries document, the way a
//! registry once declared them: a registry-level `access` and, under each
//! `packages:` key, `access`, `publish` and `unpublish` lists, the most
//! specific key winning. pnpr decides access by grants, which the database
//! holds apart from the registries, so the fixtures take the lists out of the
//! document and store the grants that give the same.

use indexmap::IndexMap;
use pnpr_config::RoutingConfig;
use pnpr_policy::{
    AccessList,
    AccessToken,
    PackageRule,
    RuleTable,
};
use pnpr_registry::{
    Ecosystem,
    PackagePattern,
};
use pnpr_storage::{
    Storage,
    database::{
        GrantContent,
        GrantWrite,
        StoredGrant,
    },
};
use serde_json::{
    Map,
    Value,
};

/// The rules of one registry of a document.
#[derive(Clone)]
pub(crate) struct Rules {
    table: RuleTable,
    /// Each `packages:` key as the document spells it, by the pattern it
    /// parses to. A grant spells the pattern the same way, because a parsed
    /// pattern does not always parse again.
    spellings: IndexMap<String, String>,
}

/// The rules of each hosted and upstream registry of a document, by key.
pub(crate) type Tables = IndexMap<String, Rules>;

/// Take the access lists out of the registries of `graph`, leaving the
/// document pnpr stores, and return the rules they declare. A registry
/// without lists gets the rules of its default grant.
pub(crate) fn take_rules(graph: &mut Value) -> Tables {
    let mut tables = Tables::new();
    let registries = graph.get_mut("registries").and_then(Value::as_object_mut);
    for (key, registry) in registries.into_iter().flatten() {
        if let Some(rules) =
            registry.as_object_mut().and_then(|registry| take_registry_rules(key, registry))
        {
            tables.insert(key.clone(), rules);
        }
    }
    tables
}

/// [`take_rules`] for one registry. A router holds no package and has none.
fn take_registry_rules(key: &str, registry: &mut Map<String, Value>) -> Option<Rules> {
    let kind = registry
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if kind != "hosted" && kind != "upstream" {
        return None;
    }
    let ecosystem: Ecosystem = registry
        .get("ecosystem")
        .cloned()
        .map(serde_json::from_value)
        .map(|ecosystem| ecosystem.expect("a known ecosystem"))
        .unwrap_or_default();
    let public = registry
        .get("public")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let default_access = registry
        .remove("access")
        .map(|access| list(&access))
        .or_else(|| {
            (kind == "upstream" && !public).then(|| AccessList::from_tokens(["$authenticated"]))
        });
    let mut rules = Vec::new();
    let mut spellings = IndexMap::new();
    let packages = registry.get_mut("packages").and_then(Value::as_object_mut);
    for (spelling, lists) in packages.into_iter().flatten() {
        let pattern = PackagePattern::parse(spelling, ecosystem)
            .unwrap_or_else(|error| panic!("registry {key:?} `packages:` key: {error}"));
        spellings.insert(pattern.to_string(), spelling.clone());
        rules.push(take_package_rule(pattern, lists));
    }
    Some(Rules { table: RuleTable::new(rules, default_access), spellings })
}

/// The rule the value of one `packages:` key declares, taking its lists out.
fn take_package_rule(pattern: PackagePattern, lists: &mut Value) -> PackageRule {
    let mut field = |name: &str| {
        let lists = lists.as_object_mut()?;
        lists.remove(name).map(|value| list(&value))
    };
    PackageRule {
        pattern,
        access: field("access"),
        publish: field("publish"),
        unpublish: field("unpublish"),
    }
}

/// A list as a document writes it: one entry, or a sequence of them.
fn list(value: &Value) -> AccessList {
    let entries: Vec<&str> = match value {
        Value::String(entry) => vec![entry.as_str()],
        Value::Array(entries) => entries
            .iter()
            .map(Value::as_str)
            .map(|text| text.expect("an access list entry"))
            .collect(),
        _ => panic!("an access list is an entry or a sequence of entries"),
    };
    AccessList::new(
        entries
            .into_iter()
            .map(AccessToken::parse)
            .collect(),
    )
}

/// Serve `tables` from `routing`, for a test that reads the rules from the
/// config it loaded.
pub(crate) fn apply(routing: &RoutingConfig, tables: &Tables) {
    for (key, Rules { table, .. }) in tables {
        let rules = routing.hosted
            .get(key)
            .map(|hosted| &hosted.rules)
            .or_else(|| routing.upstreams.get(key).map(|upstream| &upstream.rules));
        if let Some(rules) = rules {
            rules.replace(table.clone());
        }
    }
}

/// The grants that give what `tables` give, each named after its registry
/// and pattern and stored under that name as its ID.
fn rows(tables: &Tables) -> Vec<StoredGrant> {
    tables
        .iter()
        .flat_map(|(key, Rules { table, spellings })| {
            let spelled = move |patterns: Vec<String>| {
                patterns
                    .into_iter()
                    .map(|pattern| {
                        spellings
                            .get(&pattern)
                            .cloned()
                            .unwrap_or(pattern)
                    })
                    .collect::<Vec<_>>()
            };
            table
                .grants()
                .iter()
                .map(move |grant| {
                    let spec = grant.spec();
                    let name = if spec.name.is_empty() {
                        key.clone()
                    } else {
                        format!("{key}:{}", spec.name)
                    };
                    let content = GrantContent {
                        name: name.clone(),
                        registries: vec![key.clone()],
                        include: spelled(spec.include),
                        exclude: spelled(spec.exclude),
                        subjects: spec.subjects
                            .into_iter()
                            .map(|(subject, action)| (subject, action.as_str().to_string()))
                            .collect(),
                    };
                    StoredGrant { id: name, content }
                })
        })
        .collect()
}

/// Store the grants of `tables`, in place of the grants each of their
/// registries had to itself.
pub(crate) async fn store(storage: &Storage, tables: &Tables) {
    let stored = storage.read_grants(None).await.expect("read the grants");
    let stale = stored
        .into_iter()
        .flat_map(|stored| stored.grants)
        .filter(
            |grant| matches!(&grant.content.registries[..], [only] if tables.contains_key(only)),
        );
    for grant in stale {
        storage.remove_grant(&grant.id, None).await.expect("remove a replaced grant");
    }
    for grant in rows(tables) {
        let written =
            storage.put_grant(&grant.id, grant.content, None).await.expect("store a grant");
        assert!(matches!(written, GrantWrite::Written(_)), "{written:?}");
    }
}

/// The grants a `grants:` file declares, as `pnpr grants import` reads one,
/// each stored under its name as its ID.
fn declared(yaml: &str) -> Vec<StoredGrant> {
    let file: Value = serde_saphyr::from_str(yaml).expect("parse the grants file");
    let grants = file["grants"].as_object().expect("a `grants` map");
    grants
        .iter()
        .map(|(name, grant)| StoredGrant {
            id: name.clone(),
            content: declared_content(name, grant),
        })
        .collect()
}

fn declared_content(name: &str, grant: &Value) -> GrantContent {
    let subjects = grant["subjects"].as_object().expect("the subjects of a grant");
    let mut subjects: Vec<(String, String)> = subjects
        .iter()
        .flat_map(|(subject, actions)| {
            strings(actions)
                .into_iter()
                .map(move |action| (subject.clone(), action))
        })
        .collect();
    subjects.sort();
    GrantContent {
        name: name.to_string(),
        registries: strings(&grant["registries"]),
        include: strings(&grant["include"]),
        exclude: strings(&grant["exclude"]),
        subjects,
    }
}

/// The strings of a list, or none when the key is absent.
fn strings(value: &Value) -> Vec<String> {
    let entries = value.as_array().into_iter().flatten();
    entries
        .map(|entry| {
            entry
                .as_str()
                .expect("a string")
                .to_string()
        })
        .collect()
}

/// Cover the registries of `routing` with the grants the file `yaml`
/// declares, for a test that reads the rules from the routing it built.
pub(crate) fn apply_declared(routing: &RoutingConfig, yaml: &str) {
    let grants = declared(yaml);
    let tables = routing.hosted
        .iter()
        .map(|(key, hosted)| (key, &hosted.rules))
        .chain(
            routing.upstreams
                .iter()
                .map(|(key, upstream)| (key, &upstream.rules)),
        );
    for (key, rules) in tables {
        let ecosystem = routing.registries.ecosystem(key).unwrap_or_default();
        let covering = grants
            .iter()
            .filter(|grant| {
                grant.content.registries
                    .iter()
                    .any(|named| named == key || named == "*")
            })
            .map(|grant| compile(&grant.content, ecosystem))
            .collect();
        rules.set_grants(covering);
    }
}

fn compile(content: &GrantContent, ecosystem: Ecosystem) -> pnpr_policy::Grant {
    let spec = pnpr_policy::GrantSpec {
        name: content.name.clone(),
        include: content.include.clone(),
        exclude: content.exclude.clone(),
        subjects: content.subjects
            .iter()
            .map(|(subject, action)| {
                (subject.clone(), pnpr_policy::Action::parse(action).expect("a known action"))
            })
            .collect(),
    };
    pnpr_policy::Grant::compile(&spec, ecosystem).expect("a grant that applies")
}

/// Make the grants the file `yaml` declares the grants of `storage`.
pub(crate) async fn replace(storage: &Storage, yaml: &str) {
    let written = storage
        .replace_grants(declared(yaml), None)
        .await
        .expect("store the grants");
    assert!(matches!(written, GrantWrite::Written(_)), "{written:?}");
}
