use super::{
    WindowsDirSecurity, can_write_windows_dir_security, deserialize_vec_or_one,
    is_safe_windows_dir_security, is_unc_windows_path,
};
use base64::Engine as _;
use rustc_hash::{FxHashMap, FxHashSet};
use serde::Deserialize;
use std::{
    fs,
    io::Read as _,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Mutex, OnceLock, PoisonError},
    time::{Duration, Instant, SystemTime},
};

const CACHE_TTL: Duration = Duration::from_secs(60);
const POWERSHELL_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_BATCH_DIRS: usize = 64;

const POWERSHELL_SECURITY_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
$me = $null
$groups = @()
$admins = @()
if ($env:PNPM_DOCTOR_SKIP_ID -ne '1') {
  $id = [System.Security.Principal.WindowsIdentity]::GetCurrent()
  $me = $id.User.Value
  $groups = [object[]]@(@($me) + @($id.Groups | ForEach-Object { $_.Value }))
  try {
    $adminGroup = [System.DirectoryServices.DirectoryEntry]::new('WinNT://./Administrators,group')
    foreach ($member in $adminGroup.Invoke('Members')) {
      $bytes = $member.GetType().InvokeMember('objectSid', 'GetProperty', $null, $member, $null)
      $admins += [System.Security.Principal.SecurityIdentifier]::new($bytes, 0).Value
    }
  } catch {}
}
$sections = [System.Security.AccessControl.AccessControlSections]::Access -bor [System.Security.AccessControl.AccessControlSections]::Owner
$sidType = [System.Security.Principal.SecurityIdentifier]
$allowType = [System.Security.AccessControl.AccessControlType]::Allow
$items = @()
$rawPaths = if ($null -ne $env:PNPM_DOCTOR_PATHS -and $env:PNPM_DOCTOR_PATHS.Length -gt 0) { $env:PNPM_DOCTOR_PATHS -split "`n" } else { @() }
foreach ($raw in $rawPaths) {
  try {
    $dir = [System.IO.DirectoryInfo]::new($raw)
    if (-not $dir.Exists -or (($dir.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0)) {
      $items += @{ ok = $false }; continue
    }
    $sec = $dir.GetAccessControl($sections)
    $rawDesc = [System.Security.AccessControl.RawSecurityDescriptor]::new($sec.GetSecurityDescriptorBinaryForm(), 0)
    $rules = @(foreach ($rule in $sec.GetAccessRules($true, $true, $sidType)) {
      @{ sid = $rule.IdentityReference.Value; allow = ($rule.AccessControlType -eq $allowType); mask = [int]$rule.FileSystemRights; prop = [int]$rule.PropagationFlags }
    })
    $items += @{ ok = $true; nullAcl = ($null -eq $rawDesc.DiscretionaryAcl); owner = $sec.GetOwner($sidType).Value; rules = [object[]]@($rules) }
  } catch { $items += @{ ok = $false } }
}
@{ me = $me; admins = [object[]]@($admins); groups = [object[]]@($groups); items = [object[]]@($items) } | ConvertTo-Json -Depth 6 -Compress
exit 0
"#;

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

#[derive(Clone)]
struct CachedIdentity {
    current_user: String,
    admin_sids: FxHashSet<String>,
    member_sids: FxHashSet<String>,
}

#[derive(Clone, Copy)]
struct CachedDirSecurity {
    modified: Option<SystemTime>,
    expires_at: Instant,
    known: bool,
    safe: bool,
    writable: bool,
}

pub(in crate::cli_args::doctor::probe_path) fn prefetch_windows_dir_security(dirs: &[PathBuf]) {
    let valid: Vec<PathBuf> = dirs
        .iter()
        .filter(|dir| !is_unc_windows_path(dir) && is_real_windows_dir(dir))
        .cloned()
        .collect();
    if !valid.is_empty() {
        let _ = inspect_directories(&valid);
    }
}

pub(in crate::cli_args::doctor::probe_path) fn is_safe_windows_ancestor_chain(
    canonical_dir: &Path,
) -> bool {
    if is_unc_windows_path(canonical_dir) {
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

pub(in crate::cli_args::doctor::probe_path) fn can_access_windows_writable_dir(
    canonical_dir: &Path,
) -> bool {
    if !is_real_windows_dir(canonical_dir) {
        return false;
    }
    if let Some(inspected) = inspect_directories(&[canonical_dir.to_path_buf()]).first()
        && inspected.known
    {
        return inspected.writable;
    }
    is_unc_windows_path(canonical_dir)
        && fs::metadata(canonical_dir)
            .is_ok_and(|meta| meta.is_dir() && !meta.permissions().readonly())
}

fn is_real_windows_dir(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt as _;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

    fs::symlink_metadata(path)
        .is_ok_and(|meta| {
            meta.is_dir() && (meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT) == 0
        })
}

fn dir_modified_time(path: &Path) -> Option<SystemTime> {
    fs::symlink_metadata(path)
        .ok()?
        .modified()
        .ok()
}

fn security_cache() -> &'static Mutex<FxHashMap<String, CachedDirSecurity>> {
    static CACHE: OnceLock<Mutex<FxHashMap<String, CachedDirSecurity>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(FxHashMap::default()))
}

fn identity_cache() -> &'static Mutex<Option<CachedIdentity>> {
    static IDENTITY: OnceLock<Mutex<Option<CachedIdentity>>> = OnceLock::new();
    IDENTITY.get_or_init(|| Mutex::new(None))
}

fn inspect_directories(dirs: &[PathBuf]) -> Vec<CachedDirSecurity> {
    let now = Instant::now();
    let missing = collect_uncached_dirs(dirs, now);
    for chunk in missing.chunks(MAX_BATCH_DIRS) {
        populate_security_cache(chunk, now);
    }
    let guard = security_cache().lock().unwrap_or_else(PoisonError::into_inner);
    dirs.iter()
        .map(|dir| {
            guard
                .get(&cache_key(dir))
                .copied()
                .unwrap_or(CachedDirSecurity {
                    modified: None,
                    expires_at: now,
                    known: false,
                    safe: false,
                    writable: false,
                })
        })
        .collect()
}

fn collect_uncached_dirs(dirs: &[PathBuf], now: Instant) -> Vec<PathBuf> {
    let guard = security_cache().lock().unwrap_or_else(PoisonError::into_inner);
    let mut seen = FxHashSet::default();
    let mut uncached = Vec::new();
    for dir in dirs {
        let key = cache_key(dir);
        let modified = dir_modified_time(dir);
        let is_stale = guard
            .get(&key)
            .is_none_or(|entry| entry.expires_at <= now || entry.modified != modified);
        if is_stale && seen.insert(key) {
            uncached.push(dir.clone());
        }
    }
    uncached
}

fn populate_security_cache(dirs: &[PathBuf], now: Instant) {
    let has_identity = identity_cache()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .is_some();
    let Some(batch) = run_powershell_security_query(dirs, has_identity) else {
        return;
    };
    let Some(identity) = resolve_identity(&batch) else {
        return;
    };
    let mut guard = security_cache().lock().unwrap_or_else(PoisonError::into_inner);
    for (dir, item) in dirs.iter().zip(&batch.items) {
        guard.insert(
            cache_key(dir),
            CachedDirSecurity {
                modified: dir_modified_time(dir),
                expires_at: now + CACHE_TTL,
                known: item.ok,
                safe: is_safe_windows_dir_security(
                    item,
                    &identity.current_user,
                    &identity.admin_sids,
                ),
                writable: can_write_windows_dir_security(item, &identity.member_sids),
            },
        );
    }
}

fn resolve_identity(batch: &WindowsSecurityBatch) -> Option<CachedIdentity> {
    let mut guard = identity_cache().lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(current_user) = batch.me
        .as_deref()
        .filter(|sid| !sid.is_empty())
    {
        let admin_sids = batch.admins
            .iter()
            .map(String::as_str)
            .map(str::to_ascii_uppercase)
            .collect();
        let mut member_sids: FxHashSet<String> = batch.groups
            .iter()
            .map(String::as_str)
            .map(str::to_ascii_uppercase)
            .collect();
        member_sids.insert(current_user.to_ascii_uppercase());
        *guard =
            Some(CachedIdentity { current_user: current_user.to_owned(), admin_sids, member_sids });
    }
    guard.clone()
}

fn run_powershell_security_query(dirs: &[PathBuf], skip_id: bool) -> Option<WindowsSecurityBatch> {
    let payload = dirs
        .iter()
        .map(PathBuf::as_path)
        .map(Path::to_string_lossy)
        .collect::<Vec<_>>()
        .join("\n");
    let encoded = encoded_powershell_script();
    powershell_executables()
        .into_iter()
        .find_map(|executable| try_exec_powershell(&executable, encoded, &payload, skip_id))
}

fn encoded_powershell_script() -> &'static str {
    static ENCODED: OnceLock<String> = OnceLock::new();
    ENCODED.get_or_init(|| {
        let utf16le: Vec<u8> = POWERSHELL_SECURITY_SCRIPT
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        base64::engine::general_purpose::STANDARD.encode(utf16le)
    })
}

fn try_exec_powershell(
    executable: &Path,
    encoded_script: &str,
    payload: &str,
    skip_id: bool,
) -> Option<WindowsSecurityBatch> {
    let temp_dir = std::env::temp_dir();
    let mut child = Command::new(executable)
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-InputFormat",
            "None",
            "-EncodedCommand",
            encoded_script,
        ])
        .env("LOCALAPPDATA", &temp_dir)
        .env("APPDATA", &temp_dir)
        .env("PSModuleAnalysisCachePath", "NUL")
        .env("PNPM_DOCTOR_PATHS", payload)
        .env("PNPM_DOCTOR_SKIP_ID", if skip_id { "1" } else { "0" })
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let stdout_bytes = wait_for_powershell_stdout(&mut child)?;
    serde_json::from_slice(&stdout_bytes).ok()
}

fn wait_for_powershell_stdout(child: &mut Child) -> Option<Vec<u8>> {
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout.read_to_end(&mut bytes);
        bytes
    });
    let deadline = Instant::now() + POWERSHELL_TIMEOUT;
    loop {
        match child.try_wait().ok()? {
            Some(status) if status.success() => return reader.join().ok(),
            Some(_) => return None,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            None => std::thread::sleep(Duration::from_millis(25)),
        }
    }
}

fn powershell_executables() -> Vec<PathBuf> {
    let default_exe = PathBuf::from(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe");
    let mut candidates = vec![default_exe.clone()];
    if let Some(root) = std::env::var_os("SystemRoot").map(PathBuf::from)
        && root.is_absolute()
    {
        let candidate = root.join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
        if cache_key(&candidate) != cache_key(&default_exe) {
            candidates.push(candidate);
        }
    }
    candidates
}

fn cache_key(path: &Path) -> String {
    path.to_string_lossy().to_ascii_uppercase()
}
