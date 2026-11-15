#[cfg(windows)]
use rustc_hash::FxHashMap;
use rustc_hash::FxHashSet;
use serde::Deserialize;
#[cfg(windows)]
use std::{
    fs,
    io::Write as _,
    path::{Component, Path, PathBuf, Prefix},
    process::{Command, Stdio},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

const INHERIT_ONLY_PROPAGATION: i32 = 0x2;
const UNSAFE_REPLACE_MASK: u32 = 0x100d_0040;
const DIRECTORY_WRITE_MASK: u32 = 0x0000_0006;

#[cfg(windows)]
const CACHE_TTL: Duration = Duration::from_secs(2);

#[cfg(windows)]
const POWERSHELL_SECURITY_SCRIPT: &str = r#"
[Console]::InputEncoding = [System.Text.Encoding]::UTF8
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$ErrorActionPreference = 'Stop'
$id = [System.Security.Principal.WindowsIdentity]::GetCurrent()
$me = $id.User.Value
$groups = [object[]]@(@($me) + @($id.Groups | ForEach-Object { $_.Value }))
$admins = @()
try {
  $adminGroup = [System.DirectoryServices.DirectoryEntry]::new('WinNT://./Administrators,group')
  foreach ($member in $adminGroup.Invoke('Members')) {
    $bytes = $member.GetType().InvokeMember('objectSid', 'GetProperty', $null, $member, $null)
    $admins += [System.Security.Principal.SecurityIdentifier]::new($bytes, 0).Value
  }
} catch {}
$sections = [System.Security.AccessControl.AccessControlSections]::Access -bor [System.Security.AccessControl.AccessControlSections]::Owner
$sidType = [System.Security.Principal.SecurityIdentifier]
$allowType = [System.Security.AccessControl.AccessControlType]::Allow
$items = @()
foreach ($raw in [Console]::In.ReadToEnd() -split "\r?\n") {
  if ([string]::IsNullOrWhiteSpace($raw)) { continue }
  try {
    $dir = [System.IO.DirectoryInfo]::new($raw)
    if (-not $dir.Exists -or (($dir.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0)) {
      $items += @{ path = $raw; ok = $false }; continue
    }
    $sec = $dir.GetAccessControl($sections)
    $rawDesc = [System.Security.AccessControl.RawSecurityDescriptor]::new($sec.GetSecurityDescriptorBinaryForm(), 0)
    $rules = @(foreach ($rule in $sec.GetAccessRules($true, $true, $sidType)) {
      @{ sid = $rule.IdentityReference.Value; allow = ($rule.AccessControlType -eq $allowType); mask = [int]$rule.FileSystemRights; prop = [int]$rule.PropagationFlags }
    })
    $items += @{ path = $raw; ok = $true; nullAcl = ($null -eq $rawDesc.DiscretionaryAcl); owner = $sec.GetOwner($sidType).Value; rules = [object[]]@($rules) }
  } catch { $items += @{ path = $raw; ok = $false } }
}
@{ me = $me; admins = [object[]]@($admins); groups = $groups; items = [object[]]@($items) } | ConvertTo-Json -Depth 6 -Compress
"#;

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
    pub(in crate::cli_args::doctor) path: Option<String>,
    pub(in crate::cli_args::doctor) ok: bool,
    pub(in crate::cli_args::doctor) null_acl: bool,
    pub(in crate::cli_args::doctor) owner: Option<String>,
    #[serde(deserialize_with = "deserialize_vec_or_one")]
    pub(in crate::cli_args::doctor) rules: Vec<WindowsAccessRule>,
}

#[cfg(windows)]
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct WindowsSecurityBatch {
    me: Option<String>,
    #[serde(deserialize_with = "deserialize_vec_or_one")]
    admins: Vec<String>,
    #[serde(deserialize_with = "deserialize_vec_or_one")]
    groups: Vec<String>,
    #[serde(deserialize_with = "deserialize_vec_or_one")]
    items: Vec<WindowsDirSecurity>,
}

#[cfg(windows)]
#[derive(Clone, Copy)]
struct CachedDirSecurity {
    expires_at: Instant,
    safe: bool,
    writable: bool,
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
    if matches!(normalized.as_str(), "S-1-5-18" | "S-1-5-19" | "S-1-5-20" | "S-1-5-32-544") {
        return true;
    }
    if is_builtin_local_admin_sid(&normalized) {
        return true;
    }
    normalized.starts_with("S-1-5-80-")
        || normalized.starts_with("S-1-5-83-")
        || normalized.starts_with("S-1-5-90-")
}

fn is_builtin_local_admin_sid(normalized_sid: &str) -> bool {
    let Some(middle) = normalized_sid
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
) -> bool {
    let Some(owner) = security.owner.as_deref() else {
        return false;
    };
    if !security.ok || security.null_acl || security.rules.is_empty() {
        return false;
    }
    if !is_trusted_windows_principal(owner, current_user_sid, admin_sids) {
        return false;
    }
    security.rules
        .iter()
        .all(|rule| {
            !rule.allow
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

#[cfg(windows)]
pub(super) fn is_safe_windows_ancestor_chain(canonical_dir: &Path) -> bool {
    if is_unc_path(canonical_dir) {
        return false;
    }
    let ancestors: Vec<PathBuf> = canonical_dir
        .ancestors()
        .filter(|path| !path.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .collect();
    if ancestors
        .iter()
        .any(|dir| !is_real_windows_dir(dir))
    {
        return false;
    }
    inspect_directories(&ancestors).into_iter().all(|entry| entry.safe)
}

#[cfg(windows)]
pub(super) fn can_access_windows_writable_dir(canonical_dir: &Path) -> bool {
    if is_unc_path(canonical_dir) || !is_real_windows_dir(canonical_dir) {
        return false;
    }
    inspect_directories(&[canonical_dir.to_path_buf()]).first().is_some_and(|entry| entry.writable)
}

#[cfg(windows)]
fn is_unc_path(path: &Path) -> bool {
    matches!(
        path.components().next(),
        Some(Component::Prefix(prefix))
            if matches!(prefix.kind(), Prefix::UNC(..) | Prefix::VerbatimUNC(..)),
    )
}

#[cfg(windows)]
fn is_real_windows_dir(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt as _;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

    fs::symlink_metadata(path)
        .is_ok_and(|meta| {
            meta.is_dir() && (meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT) == 0
        })
}

#[cfg(windows)]
fn security_cache() -> &'static Mutex<FxHashMap<String, CachedDirSecurity>> {
    static CACHE: OnceLock<Mutex<FxHashMap<String, CachedDirSecurity>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(FxHashMap::default()))
}

#[cfg(windows)]
fn inspect_directories(dirs: &[PathBuf]) -> Vec<CachedDirSecurity> {
    let now = Instant::now();
    let missing = collect_uncached_dirs(dirs, now);
    if !missing.is_empty() {
        populate_security_cache(&missing, now);
    }
    let guard = security_cache().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    dirs.iter()
        .map(|dir| {
            guard
                .get(&cache_key(dir))
                .copied()
                .unwrap_or(CachedDirSecurity { expires_at: now, safe: false, writable: false })
        })
        .collect()
}

#[cfg(windows)]
fn collect_uncached_dirs(dirs: &[PathBuf], now: Instant) -> Vec<PathBuf> {
    let guard = security_cache().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    dirs.iter()
        .filter(|dir| {
            !guard
                .get(&cache_key(dir))
                .is_some_and(|entry| entry.expires_at > now)
        })
        .cloned()
        .collect()
}

#[cfg(windows)]
fn populate_security_cache(dirs: &[PathBuf], now: Instant) {
    let Some(batch) = run_powershell_security_query(dirs) else {
        return;
    };
    let Some(current_user) = batch.me.as_deref() else {
        return;
    };
    let admin_sids: FxHashSet<String> = batch.admins
        .iter()
        .map(|sid| sid.to_ascii_uppercase())
        .collect();
    let mut member_sids: FxHashSet<String> = batch.groups
        .iter()
        .map(|sid| sid.to_ascii_uppercase())
        .collect();
    member_sids.insert(current_user.to_ascii_uppercase());
    let mut guard = security_cache().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    for item in &batch.items {
        if let Some(raw_path) = item.path.as_deref() {
            guard.insert(
                cache_key(Path::new(raw_path)),
                CachedDirSecurity {
                    expires_at: now + CACHE_TTL,
                    safe: is_safe_windows_dir_security(item, current_user, &admin_sids),
                    writable: can_write_windows_dir_security(item, &member_sids),
                },
            );
        }
    }
}

#[cfg(windows)]
fn run_powershell_security_query(dirs: &[PathBuf]) -> Option<WindowsSecurityBatch> {
    let payload = dirs
        .iter()
        .map(|dir| dir.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("\n");
    powershell_executables()
        .into_iter()
        .find_map(|executable| try_exec_powershell(&executable, &payload))
}

#[cfg(windows)]
fn try_exec_powershell(executable: &Path, payload: &str) -> Option<WindowsSecurityBatch> {
    let temp_dir = std::env::temp_dir();
    let mut child = Command::new(executable)
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", POWERSHELL_SECURITY_SCRIPT])
        .env("LOCALAPPDATA", &temp_dir)
        .env("APPDATA", &temp_dir)
        .env("PSModuleAnalysisCachePath", "NUL")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(payload.as_bytes());
    }
    let output = child.wait_with_output().ok()?;
    if !output.status.success() {
        return None;
    }
    serde_json::from_slice(&output.stdout).ok()
}

#[cfg(windows)]
fn powershell_executables() -> Vec<PathBuf> {
    let mut candidates =
        vec![PathBuf::from(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe")];
    if let Some(system_root) = std::env::var_os("SystemRoot").map(PathBuf::from)
        && system_root.is_absolute()
    {
        let candidate = system_root.join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
        if candidate != candidates[0] {
            candidates.push(candidate);
        }
    }
    candidates
}

#[cfg(windows)]
fn cache_key(path: &Path) -> String {
    path.to_string_lossy().to_ascii_uppercase()
}
