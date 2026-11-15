import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

export interface WindowsAccessRule {
  sid: string
  allow: boolean
  mask: number
  inherit?: number
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

interface CachedIdentity {
  me: string
  adminSids: Set<string>
  memberSids: Set<string>
}

export interface WindowsSecurityIdentity {
  currentUserSid: string
  adminSids: ReadonlySet<string>
}

interface CachedDirSecurity {
  ctimeNs: bigint
  expiresAt: number
  known: boolean
  ancestorSafe: boolean
  leafSafe: boolean
  writable: boolean
}

interface TrustedRuleContext {
  currentUserSid: string
  adminSids: ReadonlySet<string>
  ownerSid: string
}

const INHERIT_ONLY_PROPAGATION = 0x2
// Rejects untrusted FILE_DELETE_CHILD (0x40), FILE_WRITE_ATTRIBUTES (0x100), DELETE (0x10000),
// WRITE_DAC (0x40000), WRITE_OWNER (0x80000), GENERIC_ALL (0x10000000), and GENERIC_WRITE (0x40000000).
const UNSAFE_REPLACE_MASK = 0x500d0140
const DIRECTORY_WRITE_MASK = 0x00000006
const CACHE_TTL_MS = 60_000
const MAX_BATCH_DIRS = 64

const TRUSTED_INSTALLER_SID = 'S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464'

const SYSTEM_TRUSTED_SIDS = new Set([
  'S-1-5-18',
  'S-1-5-32-544',
  TRUSTED_INSTALLER_SID,
])

const BUILTIN_ADMIN_USER_PATTERN = /^S-1-5-21-[\d-]+-500$/

// cspell:ignore netapi Pinvoke Winapi
const POWERSHELL_SECURITY_SCRIPT = [
  "$ErrorActionPreference = 'Stop'",
  '$me = $null',
  '$groups = @()',
  '$admins = @()',
  "if ($env:PNPM_DOCTOR_SKIP_ID -ne '1') {",
  '  $id = [System.Security.Principal.WindowsIdentity]::GetCurrent()',
  '  $me = $id.User.Value',
  '  $groups = [object[]]@(@($me) + @($id.Groups | ForEach-Object { $_.Value }))',
  '  try {',
  "    $mod = [System.AppDomain]::CurrentDomain.DefineDynamicAssembly([System.Reflection.AssemblyName]::new('PnpmSam'), [System.Reflection.Emit.AssemblyBuilderAccess]::Run).DefineDynamicModule('M')",
  "    $tb = $mod.DefineType('N', [System.Reflection.TypeAttributes]'Public,Abstract,Sealed')",
  "    $m1 = $tb.DefinePInvokeMethod('NetLocalGroupGetMembers', 'netapi32.dll', [System.Reflection.MethodAttributes]'Public,Static,PinvokeImpl', [System.Reflection.CallingConventions]::Standard, [int], [Type[]]@([IntPtr], [string], [int], [IntPtr].MakeByRefType(), [int], [int].MakeByRefType(), [int].MakeByRefType(), [IntPtr].MakeByRefType()), [System.Runtime.InteropServices.CallingConvention]::Winapi, [System.Runtime.InteropServices.CharSet]::Unicode)",
  '    $m1.SetImplementationFlags([System.Reflection.MethodImplAttributes]::PreserveSig)',
  "    $m2 = $tb.DefinePInvokeMethod('NetApiBufferFree', 'netapi32.dll', [System.Reflection.MethodAttributes]'Public,Static,PinvokeImpl', [System.Reflection.CallingConventions]::Standard, [int], [Type[]]@([IntPtr]), [System.Runtime.InteropServices.CallingConvention]::Winapi, [System.Runtime.InteropServices.CharSet]::Auto)",
  '    $m2.SetImplementationFlags([System.Reflection.MethodImplAttributes]::PreserveSig)',
  '    $sam = $tb.CreateType()',
  "    $grpName = [System.Security.Principal.SecurityIdentifier]::new('S-1-5-32-544').Translate([System.Security.Principal.NTAccount]).Value.Split('\\')[-1]",
  '    $buf = [IntPtr]::Zero; $read = 0; $tot = 0; $res = [IntPtr]::Zero',
  '    if ($sam::NetLocalGroupGetMembers([IntPtr]::Zero, $grpName, 0, [ref]$buf, -1, [ref]$read, [ref]$tot, [ref]$res) -eq 0 -and $buf -ne [IntPtr]::Zero) {',
  '      try {',
  '        for ($idx = 0; $idx -lt $read; $idx++) {',
  '          $sidPtr = [System.Runtime.InteropServices.Marshal]::ReadIntPtr($buf, $idx * [IntPtr]::Size)',
  '          if ($sidPtr -ne [IntPtr]::Zero) { $admins += [System.Security.Principal.SecurityIdentifier]::new($sidPtr).Value }',
  '        }',
  '      } finally { [void]$sam::NetApiBufferFree($buf) }',
  '    }',
  '  } catch {}',
  '}',
  '$sections = [System.Security.AccessControl.AccessControlSections]::Access -bor [System.Security.AccessControl.AccessControlSections]::Owner',
  '$sidType = [System.Security.Principal.SecurityIdentifier]',
  '$allowType = [System.Security.AccessControl.AccessControlType]::Allow',
  '$items = @()',
  '$rawPaths = if ($null -ne $env:PNPM_DOCTOR_PATHS -and $env:PNPM_DOCTOR_PATHS.Length -gt 0) { $env:PNPM_DOCTOR_PATHS -split "`n" } else { @() }',
  'foreach ($raw in $rawPaths) {',
  '  try {',
  '    $dir = [System.IO.DirectoryInfo]::new($raw)',
  '    $isReparse = ($dir.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0',
  '    if (-not $dir.Exists -or $isReparse) { $items += @{ ok = $false }; continue }',
  '    $sec = $dir.GetAccessControl($sections)',
  '    $rawDesc = [System.Security.AccessControl.RawSecurityDescriptor]::new($sec.GetSecurityDescriptorBinaryForm(), 0)',
  '    $rules = @(foreach ($rule in $sec.GetAccessRules($true, $true, $sidType)) {',
  '      @{ sid = $rule.IdentityReference.Value; allow = ($rule.AccessControlType -eq $allowType); mask = [int]$rule.FileSystemRights; prop = [int]$rule.PropagationFlags }',
  '    })',
  '    $items += @{ ok = $true; nullAcl = ($null -eq $rawDesc.DiscretionaryAcl); owner = $sec.GetOwner($sidType).Value; rules = [object[]]@($rules) }',
  '  } catch {',
  '    $items += @{ ok = $false }',
  '  }',
  '}',
  '@{ me = $me; admins = [object[]]@($admins); groups = [object[]]@($groups); items = [object[]]@($items) } | ConvertTo-Json -Depth 6 -Compress',
  'exit 0',
].join('\n')

const ENCODED_POWERSHELL_SCRIPT = Buffer.from(POWERSHELL_SECURITY_SCRIPT, 'utf16le').toString('base64')

const securityCache = new Map<string, CachedDirSecurity>()
let cachedIdentity: CachedIdentity | undefined

export function isUncWindowsPath (dirPath: string): boolean {
  return dirPath.startsWith('\\\\')
}

export function isTrustedWindowsPrincipal (
  sid: string,
  currentUserSid: string,
  adminSids: ReadonlySet<string>
): boolean {
  const normalized = sid.toUpperCase()
  if (normalized === currentUserSid.toUpperCase()) return true
  if (SYSTEM_TRUSTED_SIDS.has(normalized) || adminSids.has(normalized)) return true
  return BUILTIN_ADMIN_USER_PATTERN.test(normalized)
}

function isTrustedRuleSid (rule: WindowsAccessRule, ctx: TrustedRuleContext): boolean {
  const sid = rule.sid.toUpperCase()
  if (sid === 'S-1-3-0') {
    return (rule.prop & INHERIT_ONLY_PROPAGATION) !== 0
  }
  if (sid === 'S-1-3-4') {
    return ctx.ownerSid.toUpperCase() !== 'S-1-3-4' &&
      isTrustedWindowsPrincipal(ctx.ownerSid, ctx.currentUserSid, ctx.adminSids)
  }
  return isTrustedWindowsPrincipal(sid, ctx.currentUserSid, ctx.adminSids)
}

export function isSafeWindowsDirSecurity (
  security: WindowsDirSecurity,
  identity: WindowsSecurityIdentity,
  isProbeParent = true
): boolean {
  if (!security.ok || security.nullAcl || !security.owner || !security.rules || security.rules.length === 0) {
    return false
  }
  if (!isTrustedWindowsPrincipal(security.owner, identity.currentUserSid, identity.adminSids)) {
    return false
  }
  const ctx: TrustedRuleContext = { ...identity, ownerSid: security.owner }
  return security.rules.every((rule) =>
    !rule.allow ||
    (!isProbeParent && (rule.prop & INHERIT_ONLY_PROPAGATION) !== 0) ||
    (rule.mask & UNSAFE_REPLACE_MASK) === 0 ||
    isTrustedRuleSid(rule, ctx)
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

export function prefetchWindowsDirSecurity (dirs: string[]): void {
  if (process.platform !== 'win32' || dirs.length === 0) return
  const unique = deduplicateDirs(dirs.filter((dir) => dir !== '' && !isUncWindowsPath(dir)))
  const stats = statAvailableRealDirectories(unique)
  if (stats.dirs.length === 0) return
  inspectDirectories(stats.dirs, stats.changeTimes)
}

export function isSafeWindowsAncestorChain (canonicalDir: string, ancestors: string[]): boolean {
  if (isUncWindowsPath(canonicalDir)) return false
  const stats = statAllRealDirectories(ancestors)
  if (stats == null) return false
  const canonicalKey = normalizeCacheKey(canonicalDir)
  return inspectDirectories(ancestors, stats).every((entry, index) =>
    normalizeCacheKey(ancestors[index]) === canonicalKey ? entry.leafSafe : entry.ancestorSafe
  )
}

export function canAccessWindowsWritableDir (canonicalDir: string): boolean {
  const stats = statAllRealDirectories([canonicalDir])
  if (stats == null) return false
  const inspected = inspectDirectories([canonicalDir], stats)[0]
  if (inspected?.known) return inspected.writable
  return isUncWindowsPath(canonicalDir) && isWritableByReadOnlyStat(canonicalDir)
}

function isWritableByReadOnlyStat (dir: string): boolean {
  try {
    const stat = fs.statSync(dir)
    return stat.isDirectory() && (stat.mode & 0o200) !== 0
  } catch {
    return false
  }
}

function statAllRealDirectories (dirs: string[]): Map<string, bigint> | undefined {
  const changeTimes = new Map<string, bigint>()
  for (const dir of dirs) {
    const ctimeNs = readRealDirectoryChangeTime(dir)
    if (ctimeNs == null) return undefined
    changeTimes.set(normalizeCacheKey(dir), ctimeNs)
  }
  return changeTimes
}

function statAvailableRealDirectories (dirs: string[]): { dirs: string[], changeTimes: Map<string, bigint> } {
  const available: string[] = []
  const changeTimes = new Map<string, bigint>()
  for (const dir of dirs) {
    const ctimeNs = readRealDirectoryChangeTime(dir)
    if (ctimeNs != null) {
      available.push(dir)
      changeTimes.set(normalizeCacheKey(dir), ctimeNs)
    }
  }
  return { dirs: available, changeTimes }
}

function readRealDirectoryChangeTime (dir: string): bigint | undefined {
  try {
    const stat = fs.lstatSync(dir, { bigint: true })
    return stat.isDirectory() ? stat.ctimeNs : undefined
  } catch {
    return undefined
  }
}

function inspectDirectories (dirs: string[], changeTimes: Map<string, bigint>): CachedDirSecurity[] {
  const now = Date.now()
  const missing = deduplicateDirs(dirs.filter((dir) => !hasValidCacheEntry(dir, changeTimes, now)))
  for (let offset = 0; offset < missing.length; offset += MAX_BATCH_DIRS) {
    populateSecurityCache(missing.slice(offset, offset + MAX_BATCH_DIRS), changeTimes, now)
  }
  return dirs.map((dir) => securityCache.get(normalizeCacheKey(dir)) ?? {
    ctimeNs: 0n,
    expiresAt: 0,
    known: false,
    ancestorSafe: false,
    leafSafe: false,
    writable: false,
  })
}

function hasValidCacheEntry (dir: string, changeTimes: Map<string, bigint>, now: number): boolean {
  const key = normalizeCacheKey(dir)
  const cached = securityCache.get(key)
  const ctimeNs = changeTimes.get(key)
  return cached != null && ctimeNs != null && cached.ctimeNs === ctimeNs && cached.expiresAt > now
}

function populateSecurityCache (dirs: string[], changeTimes: Map<string, bigint>, now: number): void {
  const batch = runPowerShellSecurityQuery(dirs)
  if (batch == null) return
  const identity = resolveIdentity(batch)
  if (identity == null) return
  const secIdentity: WindowsSecurityIdentity = { currentUserSid: identity.me, adminSids: identity.adminSids }
  const items = toObjectArray<WindowsDirSecurity>(batch.items)
  for (let index = 0; index < dirs.length; index++) {
    const rawItem = items[index]
    const key = normalizeCacheKey(dirs[index])
    const ctimeNs = changeTimes.get(key)
    if (rawItem == null || ctimeNs == null) continue
    securityCache.set(key, {
      ctimeNs,
      expiresAt: now + CACHE_TTL_MS,
      known: Boolean(rawItem.ok),
      ancestorSafe: isSafeWindowsDirSecurity(rawItem, secIdentity, false),
      leafSafe: isSafeWindowsDirSecurity(rawItem, secIdentity, true),
      writable: canWriteWindowsDirSecurity(rawItem, identity.memberSids),
    })
  }
}

function resolveIdentity (batch: WindowsSecurityBatch): CachedIdentity | undefined {
  if (typeof batch.me === 'string' && batch.me !== '') {
    const adminSids = toUppercaseSet(batch.admins)
    const memberSids = toUppercaseSet(batch.groups)
    memberSids.add(batch.me.toUpperCase())
    cachedIdentity = { me: batch.me, adminSids, memberSids }
  }
  return cachedIdentity
}

function runPowerShellSecurityQuery (dirs: string[]): WindowsSecurityBatch | undefined {
  const env = buildPowerShellEnv(dirs)
  const args = ['-NoLogo', '-NoProfile', '-NonInteractive', '-InputFormat', 'None', '-EncodedCommand', ENCODED_POWERSHELL_SCRIPT]
  for (const executable of resolvePowerShellExecutables()) {
    try {
      const stdout = execFileSync(executable, args, {
        encoding: 'utf8',
        env,
        stdio: ['ignore', 'pipe', 'ignore'],
        windowsHide: true,
        timeout: 10_000,
      })
      return JSON.parse(stdout.trim()) as WindowsSecurityBatch
    } catch {
      // Try the next PowerShell executable, or fail closed if none succeeds.
    }
  }
  return undefined
}

function buildPowerShellEnv (dirs: string[]): NodeJS.ProcessEnv {
  const tempDir = os.tmpdir()
  const env: NodeJS.ProcessEnv = {}
  for (const [key, value] of Object.entries(process.env)) {
    const upper = key.toUpperCase()
    if (upper !== 'LOCALAPPDATA' && upper !== 'APPDATA' && upper !== 'PSModuleAnalysisCachePath'.toUpperCase()) {
      env[key] = value
    }
  }
  env.LOCALAPPDATA = tempDir
  env.APPDATA = tempDir
  env.PSModuleAnalysisCachePath = 'NUL'
  env.PNPM_DOCTOR_PATHS = dirs.join('\n')
  env.PNPM_DOCTOR_SKIP_ID = cachedIdentity != null ? '1' : '0'
  return env
}

function resolvePowerShellExecutables (): string[] {
  const candidates = ['C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe']
  const systemRoot = process.env.SystemRoot
  if (systemRoot && path.isAbsolute(systemRoot)) {
    candidates.push(path.join(systemRoot, 'System32', 'WindowsPowerShell', 'v1.0', 'powershell.exe'))
  }
  return deduplicateDirs(candidates)
}

function deduplicateDirs (dirs: string[]): string[] {
  const seen = new Set<string>()
  return dirs.filter((dir) => {
    const key = normalizeCacheKey(dir)
    if (seen.has(key)) return false
    seen.add(key)
    return true
  })
}

function toObjectArray<Item> (value: unknown): Item[] {
  if (Array.isArray(value)) return value as Item[]
  return value != null && typeof value === 'object' ? [value as Item] : []
}

function toUppercaseSet (value: unknown): Set<string> {
  const items = Array.isArray(value) ? value : value != null ? [value] : []
  return new Set(items.filter((item): item is string => typeof item === 'string').map((item) => item.toUpperCase()))
}

function normalizeCacheKey (dir: string): string {
  return (isUncWindowsPath(dir) ? dir : path.resolve(dir)).toUpperCase()
}
