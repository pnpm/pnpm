import crypto from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'

import { isError } from '@pnpm/error'

import type { CheckResult } from './doctor.js'

const EXCLUSIVE_PROBE_WRITE_FLAGS =
  fs.constants.O_WRONLY |
  fs.constants.O_CREAT |
  fs.constants.O_EXCL |
  (process.platform === 'win32' ? 0 : fs.constants.O_NOFOLLOW)

const PROBE_ENTRY_NAMES = ['source', 'reflink', 'hardlink', 'symlink'] as const

export interface ProbeDir {
  path: string
  dev: number
  ino: number
}

export type ProbeAncestorResolution =
  | { kind: 'ok', path: string }
  | { kind: 'unsafe' }
  | { kind: 'unavailable' }

export async function checkWritableDir (title: string, dir: string): Promise<CheckResult> {
  if (!canWriteToDir(dir)) {
    return {
      title,
      status: 'fail',
      detail: `no write access to ${dir}`,
      fix: 'Fix the directory permissions or point the setting at a writable path.',
    }
  }
  return { title, status: 'pass', detail: dir }
}

/**
 * Probe write access for `dir`: existing directories are checked for file creation directly,
 * while missing cache, store, or global bin directories probe subdirectory and file creation
 * at their deepest existing ancestor without leaving empty directories on disk.
 * When another user can modify an ancestor directory, fall back to a read-only `access` check
 * rather than creating or unlinking probe paths beneath that directory.
 */
export function canWriteToDir (dir: string): boolean {
  const resolved = resolveExistingDir(dir)
  if (resolved == null) return false
  const canonical = tryRealpathSync(resolved.path)
  if (canonical == null) return false
  if (!isSafeProbeAncestorChain(canonical)) {
    return canAccessWritableDir(canonical)
  }
  return resolved.kind === 'target' ? canCreateFile(canonical) : canCreateSubdir(canonical)
}

function canAccessWritableDir (dir: string): boolean {
  try {
    fs.accessSync(dir, fs.constants.W_OK | fs.constants.X_OK)
    return true
  } catch {
    return false
  }
}

function canCreateFile (dir: string): boolean {
  const probeFile = path.join(dir, `.pnpm-doctor-write-${process.pid}-${crypto.randomUUID()}`)
  let created = false
  try {
    const fd = fs.openSync(probeFile, EXCLUSIVE_PROBE_WRITE_FLAGS, 0o600)
    created = true
    fs.closeSync(fd)
    return true
  } catch {
    return false
  } finally {
    if (created) {
      tryUnlinkSync(probeFile)
    }
  }
}

function canCreateSubdir (ancestor: string): boolean {
  let probeDir: string | undefined
  try {
    probeDir = fs.mkdtempSync(path.join(ancestor, '.pnpm-doctor-write-'))
    return canCreateFile(probeDir)
  } catch {
    return false
  } finally {
    if (probeDir != null) {
      tryRmdirSync(probeDir)
    }
  }
}

function tryUnlinkSync (filePath: string): void {
  try {
    fs.unlinkSync(filePath)
  } catch {
    // Ignore transient cleanup errors so they never abort the doctor report.
  }
}

function tryRmdirSync (dirPath: string): void {
  try {
    fs.rmdirSync(dirPath)
  } catch {
    // Ignore transient cleanup errors so they never abort the doctor report.
  }
}

type AncestorEntryState = 'dir' | 'missing' | 'blocked'

export function existingAncestorDir (dir: string): string | undefined {
  return resolveExistingDir(dir)?.path
}

function resolveExistingDir (dir: string): { path: string, kind: 'target' | 'ancestor' } | undefined {
  if (dir === '') return undefined
  let current = path.resolve(dir)
  let kind: 'target' | 'ancestor' = 'target'
  for (;;) {
    const state = classifyPathEntry(current)
    if (state === 'dir') return { path: current, kind }
    if (state === 'blocked') return undefined
    const parent = path.dirname(current)
    if (parent === current) return undefined
    current = parent
    kind = 'ancestor'
  }
}

function classifyPathEntry (candidate: string): AncestorEntryState {
  try {
    fs.lstatSync(candidate)
  } catch (err: unknown) {
    return isMissingPathError(err) ? 'missing' : 'blocked'
  }
  return isExistingDirectory(candidate) ? 'dir' : 'blocked'
}

function isMissingPathError (err: unknown): boolean {
  return isError(err) && 'code' in err && err.code === 'ENOENT'
}

function isExistingDirectory (candidate: string): boolean {
  try {
    return fs.statSync(candidate).isDirectory()
  } catch {
    return false
  }
}

function tryRealpathSync (dir: string): string | undefined {
  try {
    return fs.realpathSync(dir)
  } catch {
    return undefined
  }
}

export function resolveProbeAncestor (dir: string): ProbeAncestorResolution {
  const ancestor = existingAncestorDir(dir)
  if (ancestor == null) return { kind: 'unavailable' }
  const canonical = tryRealpathSync(ancestor)
  if (canonical == null) return { kind: 'unavailable' }
  return isSafeProbeAncestorChain(canonical) ? { kind: 'ok', path: canonical } : { kind: 'unsafe' }
}

export function safeProbeAncestorDir (dir: string): string | undefined {
  const resolved = resolveProbeAncestor(dir)
  return resolved.kind === 'ok' ? resolved.path : undefined
}

export function ancestorsFromRoot (dir: string): string[] {
  const ancestors: string[] = []
  let current = path.resolve(dir)
  for (;;) {
    ancestors.push(current)
    const parent = path.dirname(current)
    if (parent === current) break
    current = parent
  }
  return ancestors.reverse()
}

function isSafeProbeAncestorChain (canonicalDir: string): boolean {
  if (process.platform === 'win32' || typeof process.geteuid !== 'function' || 'webcontainer' in process.versions) {
    return true
  }
  const effectiveUid = process.geteuid()
  return ancestorsFromRoot(canonicalDir).every((ancestor) => isSafeDirectoryEntry(ancestor, effectiveUid))
}

function isSafeDirectoryEntry (dir: string, effectiveUid: number): boolean {
  try {
    const stat = fs.lstatSync(dir)
    if (!stat.isDirectory()) return false
    if (stat.uid !== effectiveUid && stat.uid !== 0) return false
    return !isUnsafeWritableDirMode(stat, effectiveUid)
  } catch {
    return false
  }
}

function isUnsafeWritableDirMode (stat: fs.Stats, effectiveUid: number): boolean {
  const mode = stat.mode
  if ((mode & 0o1000) !== 0) return false
  if ((mode & 0o002) !== 0) return true
  if ((mode & 0o020) === 0 || stat.gid === 0) return false
  return !(effectiveUid !== 0 && stat.uid === effectiveUid && stat.gid === effectiveUid)
}

export async function makeProbeDir (safeAncestor: string, created: ProbeDir[]): Promise<ProbeDir> {
  const dir = await fs.promises.mkdtemp(path.join(safeAncestor, '.pnpm-doctor-'))
  const stat = await fs.promises.lstat(dir)
  if (!stat.isDirectory()) {
    throw new Error(`Probe path is not a directory: ${dir}`)
  }
  const probe = { path: dir, dev: stat.dev, ino: stat.ino }
  created.push(probe)
  return probe
}

export async function verifyProbeDir (probe: ProbeDir): Promise<void> {
  const stat = await fs.promises.lstat(probe.path)
  if (!stat.isDirectory() || stat.dev !== probe.dev || stat.ino !== probe.ino) {
    throw new Error(`Probe directory changed unexpectedly: ${probe.path}`)
  }
}

export async function writeExclusiveProbeFile (probe: ProbeDir, name: string, content: string): Promise<string> {
  await verifyProbeDir(probe)
  const filePath = path.join(probe.path, name)
  const handle = await fs.promises.open(filePath, EXCLUSIVE_PROBE_WRITE_FLAGS, 0o600)
  try {
    if (content !== '') {
      await handle.writeFile(content)
    }
  } finally {
    await handle.close()
  }
  return filePath
}

export async function removeProbeDirs (probes: ProbeDir[]): Promise<void> {
  await Promise.all(probes.map(removeSingleProbeDir))
}

async function removeSingleProbeDir (probe: ProbeDir): Promise<void> {
  try {
    await verifyProbeDir(probe)
    await Promise.all(
      PROBE_ENTRY_NAMES.map(async (name) => fs.promises.unlink(path.join(probe.path, name)).catch(() => {}))
    )
    await verifyProbeDir(probe)
    await fs.promises.rmdir(probe.path)
  } catch {
    // Ignore cleanup failures so they never abort the doctor report.
  }
}
