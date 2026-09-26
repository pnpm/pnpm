//! The `funding` manifest field, read the way npm's `libnpmfund` reads it.

use serde_json::{Map, Value};
use url::Url;

/// One funding source of a package.
#[derive(Debug, PartialEq, Eq)]
pub struct FundingSource<'a> {
    pub kind: Option<&'a str>,
    pub url: &'a str,
}

impl FundingSource<'_> {
    /// The URL without any credentials embedded in it, as it is printed and
    /// handed to the browser.
    pub fn public_url(&self) -> String {
        let Ok(mut url) = Url::parse(self.url) else {
            return self.url.to_string();
        };
        if url.username().is_empty() && url.password().is_none() {
            return self.url.to_string();
        }
        // Both fail only for URLs that cannot carry credentials.
        let _ = url.set_username("");
        let _ = url.set_password(None);
        url.into()
    }
}

/// `funding` with its string shorthands expanded to `{ "url": ... }`, the
/// shape `npm fund --json` reports.
pub fn normalize_funding(funding: &Value) -> Value {
    match funding {
        Value::Array(items) => items
            .iter()
            .map(normalize_item)
            .collect(),
        item => normalize_item(item),
    }
}

fn normalize_item(item: &Value) -> Value {
    match item {
        Value::String(url) => {
            let mut object = Map::new();
            object.insert("url".to_string(), Value::String(url.clone()));
            Value::Object(object)
        }
        other => other.clone(),
    }
}

/// Whether `npm fund` lists a package with this `funding` field: every
/// source it declares has an `http:` or `https:` URL with a host.
pub fn is_valid_funding(funding: &Value) -> bool {
    match funding {
        Value::Array(items) => {
            !items.is_empty()
                && items
                    .iter()
                    .all(|item| source_of(item).is_some())
        }
        item => source_of(item).is_some(),
    }
}

/// The sources of `funding` that carry a valid URL, in declaration order.
pub fn funding_sources(funding: &Value) -> Vec<FundingSource<'_>> {
    match funding {
        Value::Array(items) => items
            .iter()
            .filter_map(source_of)
            .collect(),
        item => source_of(item).into_iter().collect(),
    }
}

fn source_of(item: &Value) -> Option<FundingSource<'_>> {
    let (kind, url) = match item {
        Value::String(url) => (None, url.as_str()),
        Value::Object(object) => {
            (object.get("type").and_then(Value::as_str), object.get("url")?.as_str()?)
        }
        _ => return None,
    };
    is_funding_url(url).then_some(FundingSource { kind, url })
}

fn is_funding_url(url: &str) -> bool {
    Url::parse(url)
        .is_ok_and(|parsed| {
            matches!(parsed.scheme(), "http" | "https")
                && parsed
                    .host_str()
                    .is_some_and(|host| !host.is_empty())
        })
}
