import crypto from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'

import { isError } from '@pnpm/error'

import type { CheckResult } from './doctor.js'
import {
  canAccessWindowsWritableDir,
  isSafeWindowsAncestorChain,
  prefetchWindowsDirSecurity,
} from './doctorWindowsAcl.js'

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
  pinFd?: number
  pinEntry?: string
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
  if (process.platform === 'win32') return canAccessWindowsWritableDir(dir)
  try {
    fs.accessSync(dir, fs.constants.W_OK | fs.constants.X_OK)
    return true
  } catch {
    return false
  }
}

function canCreateFile (dir: string): boolean {
  const name = `.pnpm-doctor-write-${process.pid}-${crypto.randomUUID()}`
  let dirFd: number | undefined
  let entryPath = path.join(dir, name)
  let created = false
  try {
    if (HAS_PROC_SELF_FD) {
      dirFd = fs.openSync(dir, fs.constants.O_RDONLY | fs.constants.O_DIRECTORY | fs.constants.O_NOFOLLOW)
      if (!fs.fstatSync(dirFd).isDirectory()) return false
      entryPath = `/proc/self/fd/${dirFd}/${name}`
    }
    const fd = fs.openSync(entryPath, EXCLUSIVE_PROBE_WRITE_FLAGS, 0o600)
    created = true
    fs.closeSync(fd)
    return true
  } catch {
    return false
  } finally {
    if (created) tryUnlinkSync(entryPath)
    if (dirFd != null) tryCloseSync(dirFd)
  }
}

function canCreateSubdir (ancestor: string): boolean {
  let probeDir: string | undefined
  try {
    probeDir = fs.mkdtempSync(path.join(ancestor, '.pnpm-doctor-write-'))
    if (!fs.lstatSync(probeDir).isDirectory() || tryRealpathSync(probeDir) !== probeDir) return false
    return canCreateFile(probeDir)
  } catch {
    return false
  } finally {
    if (probeDir != null) tryRmdirSync(probeDir)
  }
}

function ignoreSyncError (operation: () => void): void {
  try {
    operation()
  } catch { /* Cleanup failures should never abort the doctor report. */ }
}
const tryUnlinkSync = (filePath: string): void => ignoreSyncError(() => fs.unlinkSync(filePath))
const tryRmdirSync = (dirPath: string): void => ignoreSyncError(() => fs.rmdirSync(dirPath))
const tryCloseSync = (fd: number): void => ignoreSyncError(() => fs.closeSync(fd))

type AncestorEntryState = 'dir' | 'missing' | 'blocked'

export function existingAncestorDir (dir: string): string | undefined {
  return resolveExistingDir(dir)?.path
}

interface WalkState {
  current: string
  missingDepth: number
}

function resolveExistingDir (dir: string): { path: string, kind: 'target' | 'ancestor' } | undefined {
  if (dir === '') return undefined
  const initial = initWalkOrigin(dir)
  if (initial == null) return undefined
  const state: WalkState = { current: initial.origin, missingDepth: 0 }
  for (const segment of initial.segments) {
    if (!stepPathSegment(state, segment)) return undefined
  }
  return { path: state.current, kind: state.missingDepth === 0 ? 'target' : 'ancestor' }
}

function initWalkOrigin (dir: string): { origin: string, segments: string[] } | undefined {
  if (path.isAbsolute(dir)) {
    const { root } = path.parse(dir)
    const origin = classifyPathEntry(root) === 'dir' ? tryRealpathSync(root) : undefined
    return origin != null ? { origin, segments: splitSegments(dir.slice(root.length)) } : undefined
  }
  if (process.platform === 'win32' && /^[A-Z]:/i.test(dir)) {
    const origin = tryRealpathSync(path.resolve(dir.slice(0, 2)))
    return origin != null ? { origin, segments: splitSegments(dir.slice(2)) } : undefined
  }
  const origin = tryRealpathSync(process.cwd())
  return origin != null ? { origin, segments: splitSegments(dir) } : undefined
}

function splitSegments (raw: string): string[] {
  return raw.split(/[\\/]+/).filter(Boolean)
}

function stepPathSegment (state: WalkState, segment: string): boolean {
  if (segment === '.') return true
  if (segment === '..') {
    if (state.missingDepth > 0) state.missingDepth -= 1
    else state.current = path.dirname(state.current)
    return true
  }
  if (state.missingDepth > 0) {
    state.missingDepth += 1
    return true
  }
  const candidate = path.join(state.current, segment)
  const entryState = classifyPathEntry(candidate)
  if (entryState === 'blocked') return false
  if (entryState === 'missing') {
    state.missingDepth = 1
    return true
  }
  const real = tryRealpathSync(candidate)
  if (real == null) return false
  state.current = real
  return true
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

export function prefetchDoctorCandidatePaths (candidates: Array<string | undefined>): void {
  if (process.platform !== 'win32') return
  const resolvedAncestors: string[] = []
  for (const candidate of candidates) {
    if (candidate == null || candidate === '') continue
    const existing = existingAncestorDir(candidate)
    const canonical = existing != null ? tryRealpathSync(existing) : undefined
    if (canonical != null) {
      resolvedAncestors.push(...ancestorsFromRoot(canonical))
    }
  }
  prefetchWindowsDirSecurity(resolvedAncestors)
}

export async function makeProbeDir (safeAncestor: string, created: ProbeDir[]): Promise<ProbeDir> {
  const dir = await fs.promises.mkdtemp(path.join(safeAncestor, '.pnpm-doctor-'))
  try {
    const stat = await verifyCreatedUnderAncestor(safeAncestor, dir)
    const dirFd = openProbeDirectoryFd(dir, stat)
    const pin = openWindowsPinHandle(dir)
    const probe: ProbeDir = { path: dir, dev: stat.dev, ino: stat.ino, dirFd, pinFd: pin?.fd, pinEntry: pin?.entryName, entries: [] }
    created.push(probe)
    return probe
  } catch (err) {
    tryRmdirSync(dir)
    throw err
  }
}

async function verifyCreatedUnderAncestor (safeAncestor: string, dir: string): Promise<fs.Stats> {
  const [ancestorStat, stat, canonicalDir] = await Promise.all([
    fs.promises.lstat(safeAncestor),
    fs.promises.lstat(dir),
    fs.promises.realpath(dir),
  ])
  const sameParent = path.dirname(canonicalDir) === safeAncestor &&
    path.basename(canonicalDir) === path.basename(dir)
  if (!ancestorStat.isDirectory() || !stat.isDirectory() || !sameParent) {
    throw new Error(`Probe path changed during creation: ${dir}`)
  }
  return stat
}

function openWindowsPinHandle (dir: string): { fd: number, entryName: string } | undefined {
  if (process.platform !== 'win32') return undefined
  const entryName = `.pnpm-pin-${crypto.randomUUID()}`
  const pinPath = path.join(dir, entryName)
  const fd = fs.openSync(pinPath, EXCLUSIVE_PROBE_WRITE_FLAGS, 0o600)
  if (tryRealpathSync(dir) !== dir) {
    tryCloseSync(fd)
    tryUnlinkSync(pinPath)
    throw new Error(`Probe directory redirected during pin: ${dir}`)
  }
  return { fd, entryName }
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
  return HAS_PROC_SELF_FD && probe.dirFd != null
    ? `/proc/self/fd/${probe.dirFd}/${entryName}`
    : path.join(probe.path, entryName)
}

export async function writeExclusiveProbeFile (probe: ProbeDir, prefix: string, content: string): Promise<string> {
  await verifyProbeDir(probe)
  const entryName = `.pnpm-${prefix}-${crypto.randomUUID()}`
  probe.entries.push(entryName)
  const handle = await fs.promises.open(descriptorOrJoinedPath(probe, entryName), EXCLUSIVE_PROBE_WRITE_FLAGS, 0o600)
  try {
    if (content !== '') await handle.writeFile(content)
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
    releaseWindowsPinHandle(probe)
    await verifyProbeDir(probe)
    await fs.promises.rmdir(probe.path)
  } catch {
    releaseWindowsPinHandle(probe)
  } finally {
    if (probe.dirFd != null) {
      tryCloseSync(probe.dirFd)
      probe.dirFd = undefined
    }
  }
}

function releaseWindowsPinHandle (probe: ProbeDir): void {
  if (probe.pinFd != null) {
    tryCloseSync(probe.pinFd)
    probe.pinFd = undefined
  }
  if (probe.pinEntry != null) {
    tryUnlinkSync(path.join(probe.path, probe.pinEntry))
    probe.pinEntry = undefined
  }
}
