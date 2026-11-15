import crypto from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'

import type { Config } from '@pnpm/config.reader'
import { STORE_VERSION } from '@pnpm/constants'
import { isError } from '@pnpm/error'
import { pathAbsolute } from 'path-absolute'

import type { CheckResult } from './doctor.js'

export interface LinkOperations {
  reflink: (source: string, dest: string) => boolean
  hardlink: (source: string, dest: string) => Promise<void>
}

export type FilesystemProbeOptions = Pick<Config,
| 'dir'
| 'pnpmHomeDir'
| 'storeDir'
| 'lockfileDir'
| 'workspaceDir'
| 'modulesDir'
| 'virtualStoreDir'
| 'enableGlobalVirtualStore'
| 'nodeLinker'
> & {
  benchmark?: boolean
  linkOperations?: LinkOperations
}

const defaultLinkOperations: LinkOperations = {
  reflink: canReflink,
  hardlink: async (source, dest) => fs.promises.link(source, dest),
}

export const areSameDir = (dir1: string, dir2: string): boolean =>
  dir1 !== '' && dir2 !== '' && path.relative(dir1, dir2) === ''

export function resolveConfiguredStoreDir (storeDir: string, pkgRoot: string): string {
  const storeBasePath = pathAbsolute(storeDir, pkgRoot)
  return storeBasePath.endsWith(`${path.sep}${STORE_VERSION}`)
    ? storeBasePath
    : path.join(storeBasePath, STORE_VERSION)
}

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
 */
export function canWriteToDir (dir: string): boolean {
  const resolved = resolveExistingDir(dir)
  if (resolved == null) return false
  return resolved.kind === 'target' ? canCreateFile(resolved.path) : canCreateSubdir(resolved.path)
}

function canCreateFile (dir: string): boolean {
  const probeFile = path.join(dir, `.pnpm-doctor-write-${process.pid}-${crypto.randomUUID()}`)
  try {
    const fd = fs.openSync(probeFile, 'wx', 0o600)
    fs.closeSync(fd)
    return true
  } catch {
    return false
  } finally {
    removeProbePathSync(probeFile)
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
      removeProbePathSync(probeDir)
    }
  }
}

function removeProbePathSync (probePath: string): void {
  try {
    fs.rmSync(probePath, { recursive: true, force: true })
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

interface ProbeWorkspaces {
  storeProbeDir: string
  linkProbeDir: string
  symlinkProbeDir: string
}

/**
 * Probe which link strategies work between the store and the project.
 * Reflinks and hardlinks are near-free, whereas plain copies are slower.
 */
export async function checkFilesystemCapabilities (opts: FilesystemProbeOptions): Promise<CheckResult> {
  const started = Date.now()
  const linkOps = opts.linkOperations ?? defaultLinkOperations
  const storeDir = await resolveProbeStoreDir(opts, linkOps)
  const createdProbeDirs: string[] = []
  try {
    const workspaces = await createProbeWorkspaces(opts, storeDir, createdProbeDirs)
    if (workspaces == null) {
      return probeDirectoryWarning()
    }
    const capabilities = await probeLinkCapabilities(workspaces, linkOps)
    if (capabilities == null) {
      return probeFileWarning()
    }
    return formatCapabilitiesResult(capabilities, opts.benchmark ? Date.now() - started : undefined)
  } finally {
    await removeProbeDirs(createdProbeDirs)
  }
}

function probeDirectoryWarning (): CheckResult {
  return {
    title: 'Filesystem',
    status: 'warn',
    detail: 'could not create a probe directory',
    fix: 'Check that the store and project directories are writable.',
  }
}

function probeFileWarning (): CheckResult {
  return {
    title: 'Filesystem',
    status: 'warn',
    detail: 'could not write a probe file',
    fix: 'Check that the store directory is writable.',
  }
}

function formatCapabilitiesResult (
  capabilities: { reflink: boolean, hardlink: boolean, symlink: boolean },
  durationMs: number | undefined
): CheckResult {
  const available = Object.entries(capabilities).filter(([, ok]) => ok).map(([name]) => name)
  const hasCheapLink = capabilities.reflink || capabilities.hardlink
  return {
    title: 'Filesystem',
    status: hasCheapLink ? 'pass' : 'warn',
    detail: available.length > 0 ? `available: ${available.join(', ')}` : 'only copying is available',
    fix: hasCheapLink
      ? undefined
      : 'Neither reflink nor hardlink works between the store and this project; installs will copy files and be slower. Put the store on the same filesystem as your projects.',
    durationMs,
  }
}

async function createProbeWorkspaces (
  opts: FilesystemProbeOptions,
  storeDir: string,
  createdProbeDirs: string[]
): Promise<ProbeWorkspaces | undefined> {
  const { linkTargetDir, symlinkTargetDir } = resolveProbeTargetDirs(opts, storeDir)
  const storeAncestor = existingAncestorDir(storeDir)
  const linkAncestor = existingAncestorDir(linkTargetDir)
  const symlinkAncestor = existingAncestorDir(symlinkTargetDir)
  if (storeAncestor == null || linkAncestor == null || symlinkAncestor == null) {
    return undefined
  }
  try {
    const storeProbeDir = await makeProbeDir(storeAncestor, createdProbeDirs)
    const linkProbeDir = await makeProbeDir(linkAncestor, createdProbeDirs)
    const symlinkProbeDir = areSameDir(symlinkAncestor, linkAncestor)
      ? linkProbeDir
      : await makeProbeDir(symlinkAncestor, createdProbeDirs)
    return { storeProbeDir, linkProbeDir, symlinkProbeDir }
  } catch {
    return undefined
  }
}

async function resolveProbeStoreDir (opts: FilesystemProbeOptions, linkOps: LinkOperations): Promise<string> {
  const pkgRoot = opts.workspaceDir ?? opts.dir
  if (opts.storeDir != null && opts.storeDir !== '') {
    return resolveConfiguredStoreDir(opts.storeDir, pkgRoot)
  }
  const homeStoreDir = opts.pnpmHomeDir !== '' ? path.join(opts.pnpmHomeDir, 'store', STORE_VERSION) : ''
  const homeAncestor = existingAncestorDir(homeStoreDir)
  const pkgAncestor = existingAncestorDir(pkgRoot)
  if (homeAncestor == null || pkgAncestor == null) {
    return homeStoreDir !== '' ? homeStoreDir : path.join(pkgRoot, '.pnpm-store', STORE_VERSION)
  }
  if (areSameDir(homeAncestor, pkgAncestor) || await canHardlinkBetweenAncestors(homeAncestor, pkgAncestor, linkOps)) {
    return homeStoreDir
  }
  return path.join(pkgRoot, '.pnpm-store', STORE_VERSION)
}

async function canHardlinkBetweenAncestors (
  fromAncestor: string,
  toAncestor: string,
  linkOps: LinkOperations
): Promise<boolean> {
  const created: string[] = []
  try {
    const fromProbe = await makeProbeDir(fromAncestor, created)
    const toProbe = await makeProbeDir(toAncestor, created)
    const source = path.join(fromProbe, 'source')
    await fs.promises.writeFile(source, '')
    await linkOps.hardlink(source, path.join(toProbe, 'hardlink'))
    return true
  } catch {
    return false
  } finally {
    await removeProbeDirs(created)
  }
}

function resolveProbeTargetDirs (
  opts: FilesystemProbeOptions,
  storeDir: string
): { linkTargetDir: string, symlinkTargetDir: string } {
  const modulesDirName = opts.modulesDir ?? 'node_modules'
  const projectModulesDir = pathAbsolute(modulesDirName, opts.dir)
  if (opts.nodeLinker === 'hoisted') {
    return { linkTargetDir: projectModulesDir, symlinkTargetDir: projectModulesDir }
  }
  const lockfileBaseDir = opts.lockfileDir ?? opts.dir
  if (opts.enableGlobalVirtualStore) {
    const gvsDir = opts.virtualStoreDir != null
      ? pathAbsolute(opts.virtualStoreDir, lockfileBaseDir)
      : path.join(storeDir, 'links')
    return { linkTargetDir: gvsDir, symlinkTargetDir: projectModulesDir }
  }
  const lockfileModulesDir = pathAbsolute(modulesDirName, lockfileBaseDir)
  const virtualStoreDir = opts.virtualStoreDir != null
    ? pathAbsolute(opts.virtualStoreDir, lockfileBaseDir)
    : path.join(lockfileModulesDir, '.pnpm')
  return { linkTargetDir: virtualStoreDir, symlinkTargetDir: projectModulesDir }
}

async function makeProbeDir (ancestor: string, created: string[]): Promise<string> {
  const dir = await fs.promises.mkdtemp(path.join(ancestor, '.pnpm-doctor-'))
  created.push(dir)
  return dir
}

async function removeProbeDirs (dirs: string[]): Promise<void> {
  await Promise.all(dirs.map(async (dir) => fs.promises.rm(dir, { recursive: true, force: true }).catch(() => {})))
}

async function probeLinkCapabilities (
  workspaces: ProbeWorkspaces,
  linkOps: LinkOperations
): Promise<{ reflink: boolean, hardlink: boolean, symlink: boolean } | undefined> {
  const source = path.join(workspaces.storeProbeDir, 'source')
  try {
    await fs.promises.writeFile(source, 'pnpm-doctor')
  } catch {
    return undefined
  }
  return {
    reflink: linkOps.reflink(source, path.join(workspaces.linkProbeDir, 'reflink')),
    hardlink: await canLink(() => linkOps.hardlink(source, path.join(workspaces.linkProbeDir, 'hardlink'))),
    symlink: await canLink(() => fs.promises.symlink(source, path.join(workspaces.symlinkProbeDir, 'symlink'))),
  }
}

/**
 * Clone files the same way `@pnpm/fs.indexed-pkg-importer` does.
 * Node's `COPYFILE_FICLONE_FORCE` fails with `ENOSYS` on macOS and Windows, so those platforms use `@reflink/reflink`.
 */
function canReflink (source: string, dest: string): boolean {
  try {
    if (process.platform === 'darwin' || process.platform === 'win32') {
      // eslint-disable-next-line @typescript-eslint/no-require-imports -- load the native addon synchronously only on platforms that need it
      const { reflinkFileSync } = require('@reflink/reflink') as typeof import('@reflink/reflink')
      reflinkFileSync(source, dest)
    } else {
      fs.copyFileSync(source, dest, fs.constants.COPYFILE_FICLONE_FORCE)
    }
    return true
  } catch {
    return false
  }
}

async function canLink (link: () => Promise<void>): Promise<boolean> {
  try {
    await link()
    return true
  } catch {
    return false
  }
}
