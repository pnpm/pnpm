import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

export interface WindowsAccessRule {
  sid: string
  allow: boolean
  mask: number
  inherit: number
  prop: number
}

export interface WindowsDirSecurity {
  ok: boolean
  nullAcl?: boolean
  owner?: string
  rules?: WindowsAccessRule[]
}

interface WindowsSecurityBatch {
  me?: string
  admins?: unknown
  groups?: unknown
  items?: unknown
}

interface CachedDirSecurity {
  ctimeNs: bigint
  expiresAt: number
  safe: boolean
  writable: boolean
}

const INHERIT_ONLY_PROPAGATION = 0x2
const UNSAFE_REPLACE_MASK = 0x100d0040
const DIRECTORY_WRITE_MASK = 0x00000006
const CACHE_TTL_MS = 2_000

const SYSTEM_TRUSTED_SIDS = new Set([
  'S-1-5-18',
  'S-1-5-19',
  'S-1-5-20',
  'S-1-5-32-544',
])

const BUILTIN_ADMIN_USER_PATTERN = /^S-1-5-21-[\d-]+-500$/

const POWERSHELL_SECURITY_SCRIPT = [
  '[Console]::InputEncoding = [System.Text.Encoding]::UTF8',
  '[Console]::OutputEncoding = [System.Text.Encoding]::UTF8',
  "$ErrorActionPreference = 'Stop'",
  '$id = [System.Security.Principal.WindowsIdentity]::GetCurrent()',
  '$me = $id.User.Value',
  '$groups = [object[]]@(@($me) + @($id.Groups | ForEach-Object { $_.Value }))',
  '$admins = @()',
  'try {',
  "  $adminGroup = [System.DirectoryServices.DirectoryEntry]::new('WinNT://./Administrators,group')",
  "  foreach ($member in $adminGroup.Invoke('Members')) {",
  "    $bytes = $member.GetType().InvokeMember('objectSid', 'GetProperty', $null, $member, $null)",
  '    $admins += [System.Security.Principal.SecurityIdentifier]::new($bytes, 0).Value',
  '  }',
  '} catch {}',
  '$sections = [System.Security.AccessControl.AccessControlSections]::Access -bor [System.Security.AccessControl.AccessControlSections]::Owner',
  '$sidType = [System.Security.Principal.SecurityIdentifier]',
  '$items = @()',
  'foreach ($raw in [Console]::In.ReadToEnd() -split "\\r?\\n") {',
  '  if ([string]::IsNullOrWhiteSpace($raw)) { continue }',
  '  try {',
  '    $dir = [System.IO.DirectoryInfo]::new($raw)',
  '    $isReparse = ($dir.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0',
  '    if (-not $dir.Exists -or $isReparse) { $items += @{ path = $raw; ok = $false }; continue }',
  '    $sec = $dir.GetAccessControl($sections)',
  '    $rawDesc = [System.Security.AccessControl.RawSecurityDescriptor]::new($sec.GetSecurityDescriptorBinaryForm(), 0)',
  '    $nullAcl = ($null -eq $rawDesc.DiscretionaryAcl)',
  '    $owner = $sec.GetOwner($sidType).Value',
  '    $rules = @()',
  '    foreach ($rule in $sec.GetAccessRules($true, $true, $sidType)) {',
  '      $rules += @{',
  '        sid = $rule.IdentityReference.Value',
  '        allow = ($rule.AccessControlType -eq [System.Security.AccessControl.AccessControlType]::Allow)',
  '        mask = [int]$rule.FileSystemRights',
  '        inherit = [int]$rule.InheritanceFlags',
  '        prop = [int]$rule.PropagationFlags',
  '      }',
  '    }',
  '    $items += @{ path = $raw; ok = $true; nullAcl = $nullAcl; owner = $owner; rules = [object[]]@($rules) }',
  '  } catch {',
  '    $items += @{ path = $raw; ok = $false }',
  '  }',
  '}',
  '@{ me = $me; admins = [object[]]@($admins); groups = $groups; items = [object[]]@($items) } | ConvertTo-Json -Depth 6 -Compress',
].join('\n')

const securityCache = new Map<string, CachedDirSecurity>()

export function isTrustedWindowsPrincipal (
  sid: string,
  currentUserSid: string,
  adminSids: ReadonlySet<string>
): boolean {
  const normalized = sid.toUpperCase()
  if (normalized === currentUserSid.toUpperCase()) return true
  if (SYSTEM_TRUSTED_SIDS.has(normalized) || adminSids.has(normalized)) return true
  if (BUILTIN_ADMIN_USER_PATTERN.test(normalized)) return true
  return (
    normalized.startsWith('S-1-5-80-') ||
    normalized.startsWith('S-1-5-83-') ||
    normalized.startsWith('S-1-5-90-')
  )
}

function isTrustedRuleSid (
  rule: WindowsAccessRule,
  currentUserSid: string,
  adminSids: ReadonlySet<string>,
  ownerSid: string
): boolean {
  const sid = rule.sid.toUpperCase()
  if (sid === 'S-1-3-0') {
    return (rule.prop & INHERIT_ONLY_PROPAGATION) !== 0
  }
  if (sid === 'S-1-3-4') {
    return ownerSid.toUpperCase() !== 'S-1-3-4' && isTrustedWindowsPrincipal(ownerSid, currentUserSid, adminSids)
  }
  return isTrustedWindowsPrincipal(sid, currentUserSid, adminSids)
}

export function isSafeWindowsDirSecurity (
  security: WindowsDirSecurity,
  currentUserSid: string,
  adminSids: ReadonlySet<string>
): boolean {
  if (!security.ok || security.nullAcl || !security.owner || !security.rules || security.rules.length === 0) {
    return false
  }
  if (!isTrustedWindowsPrincipal(security.owner, currentUserSid, adminSids)) {
    return false
  }
  const owner = security.owner
  return security.rules.every(
    (rule) => !rule.allow || (rule.mask & UNSAFE_REPLACE_MASK) === 0 || isTrustedRuleSid(rule, currentUserSid, adminSids, owner)
  )
}

export function canWriteWindowsDirSecurity (
  security: WindowsDirSecurity,
  memberSids: ReadonlySet<string>
): boolean {
  if (!security.ok) return false
  if (security.nullAcl) return true
  if (!security.rules) return false
  const { allowed, denied } = aggregateMemberMasks(security.rules, memberSids)
  return ((allowed & ~denied) & DIRECTORY_WRITE_MASK) === DIRECTORY_WRITE_MASK
}

function aggregateMemberMasks (
  rules: WindowsAccessRule[],
  memberSids: ReadonlySet<string>
): { allowed: number, denied: number } {
  let allowed = 0
  let denied = 0
  for (const rule of rules) {
    if ((rule.prop & INHERIT_ONLY_PROPAGATION) === 0 && memberSids.has(rule.sid.toUpperCase())) {
      if (rule.allow) allowed |= rule.mask
      else denied |= rule.mask
    }
  }
  return { allowed, denied }
}

export function isSafeWindowsAncestorChain (canonicalDir: string, ancestors: string[]): boolean {
  if (canonicalDir.startsWith('\\\\')) return false
  const stats = statRealDirectories(ancestors)
  if (stats == null) return false
  return inspectDirectories(ancestors, stats).every((entry) => entry.safe)
}

export function canAccessWindowsWritableDir (canonicalDir: string): boolean {
  if (canonicalDir.startsWith('\\\\')) return false
  const stats = statRealDirectories([canonicalDir])
  if (stats == null) return false
  return inspectDirectories([canonicalDir], stats)[0]?.writable ?? false
}

function statRealDirectories (dirs: string[]): Map<string, bigint> | undefined {
  const changeTimes = new Map<string, bigint>()
  for (const dir of dirs) {
    try {
      const stat = fs.lstatSync(dir, { bigint: true })
      if (!stat.isDirectory()) return undefined
      changeTimes.set(normalizeCacheKey(dir), stat.ctimeNs)
    } catch {
      return undefined
    }
  }
  return changeTimes
}

function inspectDirectories (dirs: string[], changeTimes: Map<string, bigint>): CachedDirSecurity[] {
  const now = Date.now()
  const missing = dirs.filter((dir) => !hasValidCacheEntry(dir, changeTimes, now))
  if (missing.length > 0) {
    populateSecurityCache(missing, changeTimes, now)
  }
  return dirs.map((dir) => securityCache.get(normalizeCacheKey(dir)) ?? {
    ctimeNs: 0n,
    expiresAt: 0,
    safe: false,
    writable: false,
  })
}

function hasValidCacheEntry (dir: string, changeTimes: Map<string, bigint>, now: number): boolean {
  const cached = securityCache.get(normalizeCacheKey(dir))
  const ctimeNs = changeTimes.get(normalizeCacheKey(dir))
  return cached != null && ctimeNs != null && cached.ctimeNs === ctimeNs && cached.expiresAt > now
}

function populateSecurityCache (dirs: string[], changeTimes: Map<string, bigint>, now: number): void {
  const batch = runPowerShellSecurityQuery(dirs)
  if (batch?.me == null) return
  const adminSids = toUppercaseSet(batch.admins)
  const memberSids = toUppercaseSet(batch.groups)
  memberSids.add(batch.me.toUpperCase())
  for (const rawItem of toObjectArray<WindowsDirSecurity & { path?: string }>(batch.items)) {
    if (typeof rawItem.path !== 'string') continue
    const key = normalizeCacheKey(rawItem.path)
    const ctimeNs = changeTimes.get(key)
    if (ctimeNs == null) continue
    securityCache.set(key, {
      ctimeNs,
      expiresAt: now + CACHE_TTL_MS,
      safe: isSafeWindowsDirSecurity(rawItem, batch.me, adminSids),
      writable: canWriteWindowsDirSecurity(rawItem, memberSids),
    })
  }
}

function runPowerShellSecurityQuery (dirs: string[]): WindowsSecurityBatch | undefined {
  const tempDir = os.tmpdir()
  const env = {
    ...process.env,
    LOCALAPPDATA: tempDir,
    APPDATA: tempDir,
    PSModuleAnalysisCachePath: 'NUL',
  }
  for (const executable of resolvePowerShellExecutables()) {
    try {
      const stdout = execFileSync(
        executable,
        ['-NoLogo', '-NoProfile', '-NonInteractive', '-Command', POWERSHELL_SECURITY_SCRIPT],
        { input: dirs.join('\n'), encoding: 'utf8', env, windowsHide: true, timeout: 10_000 }
      )
      return JSON.parse(stdout.trim()) as WindowsSecurityBatch
    } catch {
      // Try the next PowerShell executable, or fail closed if none succeeds.
    }
  }
  return undefined
}

function resolvePowerShellExecutables (): string[] {
  const candidates = [
    'C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe',
  ]
  const systemRoot = process.env.SystemRoot
  if (systemRoot && path.isAbsolute(systemRoot)) {
    candidates.push(path.join(systemRoot, 'System32', 'WindowsPowerShell', 'v1.0', 'powershell.exe'))
  }
  return Array.from(new Set(candidates))
}

function toObjectArray<Item> (value: unknown): Item[] {
  if (Array.isArray(value)) return value as Item[]
  if (value != null && typeof value === 'object') return [value as Item]
  return []
}

function toUppercaseSet (value: unknown): Set<string> {
  const items = Array.isArray(value) ? value : value != null ? [value] : []
  const result = new Set<string>()
  for (const item of items) {
    if (typeof item === 'string') {
      result.add(item.toUpperCase())
    }
  }
  return result
}

function normalizeCacheKey (dir: string): string {
  return path.resolve(dir).toUpperCase()
}
