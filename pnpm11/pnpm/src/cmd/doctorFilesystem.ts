import fs from 'node:fs'
import path from 'node:path'

import {
  type Config,
  type ConfigContext,
  createProjectModulesDirResolver,
  modulesDirOf,
} from '@pnpm/config.reader'
import { STORE_VERSION } from '@pnpm/constants'
import { safeReadProjectManifestOnly } from '@pnpm/workspace.project-manifest-reader'
import { pathAbsolute } from 'path-absolute'

import type { CheckResult } from './doctor.js'
import {
  ancestorsFromRoot,
  canWriteToDir,
  checkWritableDir,
  existingAncestorDir,
  makeProbeDir,
  type ProbeDir,
  removeProbeDirs,
  resolveProbeAncestor,
  safeProbeAncestorDir,
  verifyProbeDir,
  writeExclusiveProbeFile,
} from './doctorProbePath.js'

export { canWriteToDir, checkWritableDir, existingAncestorDir }

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
| 'packageConfigs'
| 'virtualStoreDir'
| 'enableGlobalVirtualStore'
| 'nodeLinker'
> & Partial<Pick<ConfigContext, 'rootProjectManifest' | 'rootProjectManifestDir'>> & {
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

interface ProbeWorkspaces {
  storeProbeDir: ProbeDir
  linkProbeDir: ProbeDir
  symlinkProbeDir: ProbeDir
}

type ProbeWorkspacesResult =
  | { kind: 'ok', workspaces: ProbeWorkspaces }
  | { kind: 'unsafe' }
  | { kind: 'unavailable' }

/**
 * Probe which link strategies work between the store and the project.
 * Reflinks and hardlinks are near-free, whereas plain copies are slower.
 */
export async function checkFilesystemCapabilities (opts: FilesystemProbeOptions): Promise<CheckResult> {
  const started = Date.now()
  const linkOps = opts.linkOperations ?? defaultLinkOperations
  const storeDir = await resolveProbeStoreDir(opts, linkOps)
  const createdProbeDirs: ProbeDir[] = []
  try {
    const workspaceResult = await createProbeWorkspaces(opts, storeDir, createdProbeDirs)
    if (workspaceResult.kind === 'unsafe') {
      return unsafeProbeAncestorWarning()
    }
    if (workspaceResult.kind === 'unavailable') {
      return probeDirectoryWarning()
    }
    const capabilities = await probeLinkCapabilities(workspaceResult.workspaces, linkOps)
    if (capabilities == null) {
      return probeFileWarning()
    }
    return formatCapabilitiesResult(capabilities, opts.benchmark ? Date.now() - started : undefined)
  } finally {
    await removeProbeDirs(createdProbeDirs)
  }
}

function unsafeProbeAncestorWarning (): CheckResult {
  return {
    title: 'Filesystem',
    status: 'warn',
    detail: 'cannot safely probe link support in a directory writable by other users',
    fix: 'Check ownership and permissions on the store and project ancestor directories.',
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
  createdProbeDirs: ProbeDir[]
): Promise<ProbeWorkspacesResult> {
  const projectModulesDir = await resolveProjectModulesDir(opts)
  const { linkTargetDir, symlinkTargetDir } = resolveProbeTargetDirs(opts, storeDir, projectModulesDir)
  const ancestors = resolveWorkspaceAncestors(storeDir, linkTargetDir, symlinkTargetDir)
  if (ancestors.kind !== 'ok') {
    return ancestors
  }
  try {
    const storeProbeDir = await makeProbeDir(ancestors.store, createdProbeDirs)
    const linkProbeDir = await makeProbeDir(ancestors.link, createdProbeDirs)
    const symlinkProbeDir = areSameDir(ancestors.symlink, ancestors.link)
      ? linkProbeDir
      : await makeProbeDir(ancestors.symlink, createdProbeDirs)
    return { kind: 'ok', workspaces: { storeProbeDir, linkProbeDir, symlinkProbeDir } }
  } catch {
    return { kind: 'unavailable' }
  }
}

function resolveWorkspaceAncestors (
  storeDir: string,
  linkTargetDir: string,
  symlinkTargetDir: string
): { kind: 'ok', store: string, link: string, symlink: string } | { kind: 'unsafe' } | { kind: 'unavailable' } {
  const store = resolveProbeAncestor(storeDir)
  const link = resolveProbeAncestor(linkTargetDir)
  const symlink = resolveProbeAncestor(symlinkTargetDir)
  if (store.kind === 'unavailable' || link.kind === 'unavailable' || symlink.kind === 'unavailable') {
    return { kind: 'unavailable' }
  }
  if (store.kind === 'unsafe' || link.kind === 'unsafe' || symlink.kind === 'unsafe') {
    return { kind: 'unsafe' }
  }
  return { kind: 'ok', store: store.path, link: link.path, symlink: symlink.path }
}

async function resolveProjectModulesDir (opts: FilesystemProbeOptions): Promise<string> {
  if (opts.lockfileDir != null) {
    return modulesDirOf(opts.dir, opts.modulesDir)
  }
  const manifest = opts.dir === opts.rootProjectManifestDir && opts.rootProjectManifest != null
    ? opts.rootProjectManifest
    : await safeReadProjectManifestOnly(opts.dir) ?? undefined
  return modulesDirOf(opts.dir, createProjectModulesDirResolver(opts)(manifest?.name))
}

async function resolveProbeStoreDir (opts: FilesystemProbeOptions, linkOps: LinkOperations): Promise<string> {
  const pkgRoot = opts.workspaceDir ?? opts.dir
  if (opts.storeDir != null && opts.storeDir !== '') {
    return resolveConfiguredStoreDir(opts.storeDir, pkgRoot)
  }
  const homeStoreDir = opts.pnpmHomeDir !== '' ? path.join(opts.pnpmHomeDir, 'store', STORE_VERSION) : ''
  const fallbackDefault = homeStoreDir !== '' ? homeStoreDir : path.join(pkgRoot, '.pnpm-store', STORE_VERSION)
  const pkgAncestor = safeProbeAncestorDir(pkgRoot)
  if (pkgAncestor == null) {
    return fallbackDefault
  }
  if (await canUseHomeStoreDir(opts.pnpmHomeDir, pkgAncestor, linkOps)) {
    return homeStoreDir
  }
  const mountRoot = await findLinkableMountRoot(pkgAncestor, linkOps)
  return mountRoot != null ? path.join(mountRoot, '.pnpm-store', STORE_VERSION) : fallbackDefault
}

async function canUseHomeStoreDir (
  pnpmHomeDir: string,
  pkgAncestor: string,
  linkOps: LinkOperations
): Promise<boolean> {
  const homeAncestor = safeProbeAncestorDir(pnpmHomeDir)
  if (homeAncestor == null) return false
  if (areSameDir(homeAncestor, pkgAncestor)) return true
  return canHardlinkBetweenAncestors(homeAncestor, pkgAncestor, linkOps)
}

async function findLinkableMountRoot (pkgAncestor: string, linkOps: LinkOperations): Promise<string | undefined> {
  const created: ProbeDir[] = []
  try {
    const pkgProbe = await makeProbeDir(pkgAncestor, created)
    const source = await writeExclusiveProbeFile(pkgProbe, 'source', '')
    const candidates = ancestorsFromRoot(pkgAncestor)
    const linkable = await Promise.all(candidates.map(async (candidate) => canHardlinkToAncestor(source, candidate, linkOps)))
    return candidates.find((_, index) => linkable[index])
  } catch {
    return undefined
  } finally {
    await removeProbeDirs(created)
  }
}

async function canHardlinkBetweenAncestors (
  fromSafeAncestor: string,
  toSafeAncestor: string,
  linkOps: LinkOperations
): Promise<boolean> {
  const created: ProbeDir[] = []
  try {
    const fromProbe = await makeProbeDir(fromSafeAncestor, created)
    const source = await writeExclusiveProbeFile(fromProbe, 'source', '')
    return await canHardlinkToAncestor(source, toSafeAncestor, linkOps)
  } catch {
    return false
  } finally {
    await removeProbeDirs(created)
  }
}

async function canHardlinkToAncestor (source: string, toSafeAncestor: string, linkOps: LinkOperations): Promise<boolean> {
  const created: ProbeDir[] = []
  try {
    const toProbe = await makeProbeDir(toSafeAncestor, created)
    await verifyProbeDir(toProbe)
    await linkOps.hardlink(source, path.join(toProbe.path, 'hardlink'))
    return true
  } catch {
    return false
  } finally {
    await removeProbeDirs(created)
  }
}

function resolveProbeTargetDirs (
  opts: FilesystemProbeOptions,
  storeDir: string,
  projectModulesDir: string
): { linkTargetDir: string, symlinkTargetDir: string } {
  const lockfileBaseDir = opts.lockfileDir ?? opts.dir
  const lockfileModulesDir = opts.lockfileDir != null
    ? modulesDirOf(opts.lockfileDir, opts.modulesDir)
    : projectModulesDir
  if (opts.nodeLinker === 'hoisted') {
    return { linkTargetDir: lockfileModulesDir, symlinkTargetDir: projectModulesDir }
  }
  if (opts.enableGlobalVirtualStore) {
    const gvsDir = opts.virtualStoreDir != null
      ? pathAbsolute(opts.virtualStoreDir, lockfileBaseDir)
      : path.join(storeDir, 'links')
    return { linkTargetDir: gvsDir, symlinkTargetDir: projectModulesDir }
  }
  const virtualStoreDir = opts.virtualStoreDir != null
    ? pathAbsolute(opts.virtualStoreDir, lockfileBaseDir)
    : path.join(lockfileModulesDir, '.pnpm')
  return { linkTargetDir: virtualStoreDir, symlinkTargetDir: projectModulesDir }
}

async function probeLinkCapabilities (
  workspaces: ProbeWorkspaces,
  linkOps: LinkOperations
): Promise<{ reflink: boolean, hardlink: boolean, symlink: boolean } | undefined> {
  let source: string
  try {
    source = await writeExclusiveProbeFile(workspaces.storeProbeDir, 'source', 'pnpm-doctor')
    await verifyProbeDir(workspaces.linkProbeDir)
    await verifyProbeDir(workspaces.symlinkProbeDir)
  } catch {
    return undefined
  }
  return {
    reflink: linkOps.reflink(source, path.join(workspaces.linkProbeDir.path, 'reflink')),
    hardlink: await canLink(() => linkOps.hardlink(source, path.join(workspaces.linkProbeDir.path, 'hardlink'))),
    symlink: await canLink(() => fs.promises.symlink(source, path.join(workspaces.symlinkProbeDir.path, 'symlink'))),
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
