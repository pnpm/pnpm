#[cfg(windows)]
pub(super) use powershell::{
    can_access_windows_writable_dir, is_safe_windows_ancestor_chain, prefetch_windows_dir_security,
};

use rustc_hash::FxHashSet;
use serde::Deserialize;
use std::path::{Component, Path, Prefix};

#[cfg(windows)]
mod powershell;

const INHERIT_ONLY_PROPAGATION: i32 = 0x2;
const UNSAFE_REPLACE_MASK: u32 = 0x500d_0140;
const DIRECTORY_WRITE_MASK: u32 = 0x0000_0006;
const TRUSTED_INSTALLER_SID: &str =
    "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464";

#[derive(Deserialize)]
#[serde(untagged)]
enum OneOrMany<Element> {
    Many(Vec<Element>),
    One(Element),
}

fn deserialize_vec_or_one<'de, DeserializerState, Element>(
    deserializer: DeserializerState,
) -> Result<Vec<Element>, DeserializerState::Error>
where
    DeserializerState: serde::Deserializer<'de>,
    Element: Deserialize<'de>,
{
    Ok(match Option::<OneOrMany<Element>>::deserialize(deserializer)? {
        Some(OneOrMany::Many(items)) => items,
        Some(OneOrMany::One(item)) => vec![item],
        None => Vec::new(),
    })
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(in crate::cli_args::doctor) struct WindowsAccessRule {
    pub(in crate::cli_args::doctor) sid: String,
    pub(in crate::cli_args::doctor) allow: bool,
    pub(in crate::cli_args::doctor) mask: i32,
    pub(in crate::cli_args::doctor) prop: i32,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(in crate::cli_args::doctor) struct WindowsDirSecurity {
    pub(in crate::cli_args::doctor) ok: bool,
    pub(in crate::cli_args::doctor) null_acl: bool,
    pub(in crate::cli_args::doctor) owner: Option<String>,
    #[serde(deserialize_with = "deserialize_vec_or_one")]
    pub(in crate::cli_args::doctor) rules: Vec<WindowsAccessRule>,
}

pub(in crate::cli_args::doctor) fn is_unc_windows_path(path: &Path) -> bool {
    matches!(
        path.components().next(),
        Some(Component::Prefix(prefix))
            if matches!(prefix.kind(), Prefix::UNC(..) | Prefix::VerbatimUNC(..)),
    ) || path.to_string_lossy().starts_with(r"\\")
}

pub(in crate::cli_args::doctor) fn is_trusted_windows_principal(
    sid: &str,
    current_user_sid: &str,
    admin_sids: &FxHashSet<String>,
) -> bool {
    let normalized = sid.to_ascii_uppercase();
    if normalized == current_user_sid.to_ascii_uppercase() || admin_sids.contains(&normalized) {
        return true;
    }
    if matches!(normalized.as_str(), "S-1-5-18" | "S-1-5-32-544" | TRUSTED_INSTALLER_SID) {
        return true;
    }
    is_builtin_admin_user_sid(&normalized)
}

fn is_builtin_admin_user_sid(upper_sid: &str) -> bool {
    let Some(middle) = upper_sid
        .strip_prefix("S-1-5-21-")
        .and_then(|rest| rest.strip_suffix("-500"))
    else {
        return false;
    };
    !middle.is_empty()
        && middle
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'-')
}

fn is_trusted_rule_sid(
    rule: &WindowsAccessRule,
    current_user_sid: &str,
    admin_sids: &FxHashSet<String>,
    owner_sid: &str,
) -> bool {
    let sid = rule.sid.to_ascii_uppercase();
    if sid == "S-1-3-0" {
        return (rule.prop & INHERIT_ONLY_PROPAGATION) != 0;
    }
    if sid == "S-1-3-4" {
        return !owner_sid.eq_ignore_ascii_case("S-1-3-4")
            && is_trusted_windows_principal(owner_sid, current_user_sid, admin_sids);
    }
    is_trusted_windows_principal(&sid, current_user_sid, admin_sids)
}

pub(in crate::cli_args::doctor) fn is_safe_windows_dir_security(
    security: &WindowsDirSecurity,
    current_user_sid: &str,
    admin_sids: &FxHashSet<String>,
    is_probe_parent: bool,
) -> bool {
    if !security.ok || security.null_acl || security.rules.is_empty() {
        return false;
    }
    let Some(owner) = security.owner.as_deref() else {
        return false;
    };
    if !is_trusted_windows_principal(owner, current_user_sid, admin_sids) {
        return false;
    }
    security.rules
        .iter()
        .all(|rule| {
            !rule.allow
                || (!is_probe_parent && (rule.prop & INHERIT_ONLY_PROPAGATION) != 0)
                || (rule.mask.cast_unsigned() & UNSAFE_REPLACE_MASK) == 0
                || is_trusted_rule_sid(rule, current_user_sid, admin_sids, owner)
        })
}

pub(in crate::cli_args::doctor) fn can_write_windows_dir_security(
    security: &WindowsDirSecurity,
    member_sids: &FxHashSet<String>,
) -> bool {
    if !security.ok {
        return false;
    }
    if security.null_acl {
        return true;
    }
    let (allowed, denied) = aggregate_member_masks(&security.rules, member_sids);
    ((allowed & !denied) & DIRECTORY_WRITE_MASK) == DIRECTORY_WRITE_MASK
}

fn aggregate_member_masks(
    rules: &[WindowsAccessRule],
    member_sids: &FxHashSet<String>,
) -> (u32, u32) {
    let mut allowed = 0u32;
    let mut denied = 0u32;
    for rule in rules {
        if (rule.prop & INHERIT_ONLY_PROPAGATION) == 0
            && member_sids.contains(&rule.sid.to_ascii_uppercase())
        {
            if rule.allow {
                allowed |= rule.mask.cast_unsigned();
            } else {
                denied |= rule.mask.cast_unsigned();
            }
        }
    }
    (allowed, denied)
}
