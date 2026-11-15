import fs from 'node:fs'
import path from 'node:path'

import type { Config } from '@pnpm/config.reader'
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
 * Probe write access for `dir` at its deepest existing ancestor directory.
 * Missing cache, store, or global bin directories pass on a fresh environment without creating empty directories.
 */
export function canWriteToDir (dir: string): boolean {
  const ancestor = existingAncestorDir(dir)
  if (ancestor == null) return false
  let probeDir: string | undefined
  try {
    probeDir = fs.mkdtempSync(path.join(ancestor, '.pnpm-doctor-write-'))
    return true
  } catch {
    return false
  } finally {
    if (probeDir != null) {
      fs.rmSync(probeDir, { recursive: true, force: true })
    }
  }
}

type AncestorEntryState = 'dir' | 'missing' | 'blocked'

export function existingAncestorDir (dir: string): string | undefined {
  if (dir === '') return undefined
  let current = path.resolve(dir)
  for (;;) {
    const state = classifyPathEntry(current)
    if (state === 'dir') return current
    if (state === 'blocked') return undefined
    const parent = path.dirname(current)
    if (parent === current) return undefined
    current = parent
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
    return pathAbsolute(opts.storeDir, pkgRoot)
  }
  const homeAncestor = existingAncestorDir(opts.pnpmHomeDir)
  const pkgAncestor = existingAncestorDir(pkgRoot)
  if (homeAncestor == null || pkgAncestor == null) {
    return opts.pnpmHomeDir !== '' ? opts.pnpmHomeDir : pkgRoot
  }
  if (areSameDir(homeAncestor, pkgAncestor) || await canHardlinkBetweenAncestors(homeAncestor, pkgAncestor, linkOps)) {
    return path.join(opts.pnpmHomeDir, 'store')
  }
  return pkgRoot
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
  const modulesBaseDir = opts.lockfileDir ?? opts.workspaceDir ?? opts.dir
  const modulesDir = pathAbsolute(opts.modulesDir ?? 'node_modules', modulesBaseDir)
  if (opts.nodeLinker === 'hoisted') {
    return { linkTargetDir: modulesDir, symlinkTargetDir: modulesDir }
  }
  if (opts.enableGlobalVirtualStore) {
    const gvsDir = opts.virtualStoreDir != null
      ? pathAbsolute(opts.virtualStoreDir, modulesBaseDir)
      : path.join(storeDir, 'links')
    return { linkTargetDir: gvsDir, symlinkTargetDir: modulesDir }
  }
  const virtualStoreDir = opts.virtualStoreDir != null
    ? pathAbsolute(opts.virtualStoreDir, modulesBaseDir)
    : path.join(modulesDir, '.pnpm')
  return { linkTargetDir: virtualStoreDir, symlinkTargetDir: modulesDir }
}

async function makeProbeDir (ancestor: string, created: string[]): Promise<string> {
  const dir = await fs.promises.mkdtemp(path.join(ancestor, '.pnpm-doctor-'))
  created.push(dir)
  return dir
}

async function removeProbeDirs (dirs: string[]): Promise<void> {
  await Promise.all(dirs.map((dir) => fs.promises.rm(dir, { recursive: true, force: true })))
}

async function probeLinkCapabilities (
  workspaces: ProbeWorkspaces,
  linkOps: LinkOperations
): Promise<{ reflink: boolean, hardlink: boolean, symlink: boolean }> {
  const source = path.join(workspaces.storeProbeDir, 'source')
  await fs.promises.writeFile(source, 'pnpm-doctor')
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
