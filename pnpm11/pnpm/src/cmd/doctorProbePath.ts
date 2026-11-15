import crypto from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'

import { isError } from '@pnpm/error'

import type { CheckResult } from './doctor.js'
import { canAccessWindowsWritableDir, isSafeWindowsAncestorChain } from './doctorWindowsAcl.js'

const EXCLUSIVE_PROBE_WRITE_FLAGS =
  fs.constants.O_WRONLY |
  fs.constants.O_CREAT |
  fs.constants.O_EXCL |
  (process.platform === 'win32' ? 0 : fs.constants.O_NOFOLLOW)

const HAS_PROC_SELF_FD = process.platform === 'linux' && fs.existsSync('/proc/self/fd')

export interface ProbeDir {
  path: string
  dev: number
  ino: number
  dirFd?: number
  entries: string[]
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
 * When another user can modify an ancestor directory, fall back to a read-only check
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
  if (process.platform === 'win32') {
    return canAccessWindowsWritableDir(dir)
  }
  try {
    fs.accessSync(dir, fs.constants.W_OK | fs.constants.X_OK)
    return true
  } catch {
    return false
  }
}

function canCreateFile (dir: string): boolean {
  if (HAS_PROC_SELF_FD) {
    return canCreateFileViaProcFd(dir)
  }
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

function canCreateFileViaProcFd (dir: string): boolean {
  let dirFd: number | undefined
  let entryPath: string | undefined
  let created = false
  try {
    dirFd = fs.openSync(dir, fs.constants.O_RDONLY | fs.constants.O_DIRECTORY | fs.constants.O_NOFOLLOW)
    if (!fs.fstatSync(dirFd).isDirectory()) return false
    entryPath = `/proc/self/fd/${dirFd}/.pnpm-doctor-write-${process.pid}-${crypto.randomUUID()}`
    const fileFd = fs.openSync(entryPath, EXCLUSIVE_PROBE_WRITE_FLAGS, 0o600)
    created = true
    fs.closeSync(fileFd)
    return true
  } catch {
    return false
  } finally {
    if (created && entryPath != null) {
      tryUnlinkSync(entryPath)
    }
    if (dirFd != null) {
      tryCloseSync(dirFd)
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

function tryCloseSync (fd: number): void {
  try {
    fs.closeSync(fd)
  } catch {
    // Ignore descriptor close errors during cleanup.
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
  if ('webcontainer' in process.versions) {
    return true
  }
  if (process.platform === 'win32') {
    return isSafeWindowsAncestorChain(canonicalDir, ancestorsFromRoot(canonicalDir))
  }
  if (typeof process.geteuid !== 'function') {
    return false
  }
  const effectiveUid = process.geteuid()
  return ancestorsFromRoot(canonicalDir).every((ancestor) => isSafeDirectoryEntry(ancestor, effectiveUid))
}

function isSafeDirectoryEntry (dir: string, effectiveUid: number): boolean {
  try {
    const stat = fs.lstatSync(dir)
    if (!stat.isDirectory()) return false
    if (stat.uid !== effectiveUid && stat.uid !== 0) return false
    return !isUnsafeWritableDirMode(stat.mode)
  } catch {
    return false
  }
}

export function isUnsafeWritableDirMode (mode: number): boolean {
  return (mode & 0o1000) === 0 && (mode & 0o022) !== 0
}

export async function makeProbeDir (safeAncestor: string, created: ProbeDir[]): Promise<ProbeDir> {
  const dir = await fs.promises.mkdtemp(path.join(safeAncestor, '.pnpm-doctor-'))
  const stat = await fs.promises.lstat(dir)
  if (!stat.isDirectory()) {
    throw new Error(`Probe path is not a directory: ${dir}`)
  }
  const dirFd = openProbeDirectoryFd(dir, stat)
  const probe: ProbeDir = { path: dir, dev: stat.dev, ino: stat.ino, dirFd, entries: [] }
  created.push(probe)
  return probe
}

function openProbeDirectoryFd (dir: string, expected: fs.Stats): number | undefined {
  if (process.platform === 'win32') return undefined
  const fd = fs.openSync(dir, fs.constants.O_RDONLY | fs.constants.O_DIRECTORY | fs.constants.O_NOFOLLOW)
  const opened = fs.fstatSync(fd)
  const validOwner = typeof process.geteuid !== 'function' || opened.uid === process.geteuid()
  if (!opened.isDirectory() || opened.dev !== expected.dev || opened.ino !== expected.ino || !validOwner) {
    tryCloseSync(fd)
    throw new Error(`Probe directory changed during open: ${dir}`)
  }
  return fd
}

export async function verifyProbeDir (probe: ProbeDir): Promise<void> {
  const stat = await fs.promises.lstat(probe.path)
  if (!stat.isDirectory() || stat.dev !== probe.dev || stat.ino !== probe.ino) {
    throw new Error(`Probe directory changed unexpectedly: ${probe.path}`)
  }
  if (process.platform === 'win32' && await fs.promises.realpath(probe.path) !== probe.path) {
    throw new Error(`Probe directory canonical path changed: ${probe.path}`)
  }
}

export async function prepareProbeEntryPath (probe: ProbeDir, prefix: string): Promise<string> {
  await verifyProbeDir(probe)
  const entryName = `.pnpm-${prefix}-${crypto.randomUUID()}`
  probe.entries.push(entryName)
  return path.join(probe.path, entryName)
}

function descriptorOrJoinedPath (probe: ProbeDir, entryName: string): string {
  if (HAS_PROC_SELF_FD && probe.dirFd != null) {
    return `/proc/self/fd/${probe.dirFd}/${entryName}`
  }
  return path.join(probe.path, entryName)
}

export async function writeExclusiveProbeFile (probe: ProbeDir, prefix: string, content: string): Promise<string> {
  await verifyProbeDir(probe)
  const entryName = `.pnpm-${prefix}-${crypto.randomUUID()}`
  probe.entries.push(entryName)
  const handle = await fs.promises.open(descriptorOrJoinedPath(probe, entryName), EXCLUSIVE_PROBE_WRITE_FLAGS, 0o600)
  try {
    if (content !== '') {
      await handle.writeFile(content)
    }
  } finally {
    await handle.close()
  }
  return path.join(probe.path, entryName)
}

export async function removeProbeDirs (probes: ProbeDir[]): Promise<void> {
  await Promise.all(probes.map(removeSingleProbeDir))
}

async function removeSingleProbeDir (probe: ProbeDir): Promise<void> {
  try {
    await verifyProbeDir(probe)
    await Promise.all(
      probe.entries.map(async (entryName) => fs.promises.unlink(descriptorOrJoinedPath(probe, entryName)).catch(() => {}))
    )
    await verifyProbeDir(probe)
    await fs.promises.rmdir(probe.path)
  } catch {
    // Ignore cleanup failures so they never abort the doctor report.
  } finally {
    if (probe.dirFd != null) {
      tryCloseSync(probe.dirFd)
      probe.dirFd = undefined
    }
  }
}
