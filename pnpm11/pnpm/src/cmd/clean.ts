import { promises as fs, type Stats } from 'node:fs'
import path, { type ParsedPath } from 'node:path'

import { docsUrl } from '@pnpm/cli.utils'
import { types as allTypes } from '@pnpm/config.reader'
import { isError } from '@pnpm/error'
import type { ProjectsGraph } from '@pnpm/types'
import { findWorkspaceProjectsNoCheck } from '@pnpm/workspace.projects-reader'
import { readWorkspaceManifest } from '@pnpm/workspace.workspace-manifest-reader'
import { rimraf } from '@zkochan/rimraf'
import { isSubdir, strict as isStrictSubdir } from 'is-subdir'
import { pick } from 'ramda'
import { renderHelp } from 'render-help'

export const commandNames = ['clean', 'purge']

export const overridableByScript = true

export const rcOptionsTypes = (): Record<string, unknown> =>
  pick([
    'lockfile-dir',
    'modules-dir',
    'shared-workspace-lockfile',
    'virtual-store-dir',
  ], allTypes)

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...rcOptionsTypes(),
    lockfile: Boolean,
    recursive: Boolean,
  }
}

export const shorthands: Record<string, string> = {
  l: '--lockfile',
}

export function help (): string {
  return renderHelp({
    aliases: ['purge'],
    description: 'Safely remove node_modules directories from all workspace projects. \
Uses Node.js to remove directories, which correctly handles NTFS junctions on Windows \
without following them into their targets. \
If the current project has a "clean" (or "purge") script in package.json, \
the script is executed instead of the built-in command.',
    descriptionLists: [
      {
        title: 'Options',
        list: [
          {
            description: 'Also remove pnpm-lock.yaml files',
            name: '--lockfile',
            shortAlias: '-l',
          },
        ],
      },
    ],
    url: docsUrl('clean'),
    usages: ['pnpm clean [--lockfile]'],
  })
}

export interface CleanOptions {
  cliOptions?: {
    lockfile?: boolean
    'lockfile-dir'?: string
  }
  dir: string
  filter?: string[]
  filterProd?: string[]
  lockfileDir?: string
  modulesDir?: string
  modulesDirsByProjectName?: Record<string, string>
  selectedProjectsGraph?: ProjectsGraph
  virtualStoreDir?: string
  workspaceDir?: string
  workspacePackagePatterns?: string[]
  workspaceRoot?: boolean
}

export async function handler (opts: CleanOptions): Promise<void> {
  await cleanProjects({ ...opts, removeLockfile: opts.cliOptions?.lockfile === true })
}

interface CleanProjectContext {
  cleanedModulesDirs: Set<string>
  cwd: string
  modulesDir: string
  realRootDir: string
  removedLockfiles: Set<string>
  removeLockfile: boolean
}

export async function cleanProjects (opts: CleanOptions & { removeLockfile: boolean }): Promise<void> {
  const modulesDir = opts.modulesDir ?? 'node_modules'
  const rootDir = opts.workspaceDir ?? opts.dir
  const realRootDir = await tryRealpath(rootDir)
  if (realRootDir == null) return
  const cwd = await tryRealpath(process.cwd()) ?? process.cwd()
  const narrowed = isNarrowedRun(opts)
  const cleanOpts: CleanProjectContext = {
    cleanedModulesDirs: new Set(),
    cwd,
    modulesDir,
    realRootDir,
    removedLockfiles: new Set(),
    removeLockfile: opts.removeLockfile,
  }
  const dirs = await getProjectDirs(opts, { narrowed, realRootDir, rootDir })
  for (const dir of dirs) {
    // eslint-disable-next-line no-await-in-loop -- deduplicates targets across aliased directories deterministically
    await cleanProjectDir(cleanOpts, dir)
  }
  await cleanConfiguredLockfileDir(opts, cleanOpts, { dirs, narrowed, rootDir })
  if (!narrowed && opts.virtualStoreDir) {
    await removeExternalVirtualStore({
      cwd,
      modulesDir,
      rootDir,
      virtualStoreDir: opts.virtualStoreDir,
    })
  }
}

function isNarrowedRun (opts: Pick<CleanOptions, 'filter' | 'filterProd' | 'workspaceRoot'>): boolean {
  return Boolean(opts.filter?.length || opts.filterProd?.length || opts.workspaceRoot)
}

async function cleanConfiguredLockfileDir (
  opts: CleanOptions,
  cleanOpts: CleanProjectContext,
  { dirs, narrowed, rootDir }: { dirs: string[], narrowed: boolean, rootDir: string }
): Promise<void> {
  const cleansRoot = !narrowed || await dirsIncludeRoot(dirs, cleanOpts.realRootDir)
  if (!cleansRoot) return
  const resolvedLockfileDir = await resolveConfiguredLockfileDir(opts, rootDir, cleanOpts.realRootDir)
  if (resolvedLockfileDir == null) return
  if (!narrowed) {
    const lockfileModulesDir = await resolveProjectModulesDir({
      modulesDir: cleanOpts.modulesDir,
      projectDir: resolvedLockfileDir,
      realRootDir: cleanOpts.realRootDir,
    })
    if (lockfileModulesDir != null && !cleanOpts.cleanedModulesDirs.has(lockfileModulesDir)) {
      cleanOpts.cleanedModulesDirs.add(lockfileModulesDir)
      await cleanModulesDir(cleanOpts.cwd, lockfileModulesDir)
    }
  }
  if (cleanOpts.removeLockfile) {
    await removeProjectLockfile(cleanOpts, resolvedLockfileDir)
  }
}

async function dirsIncludeRoot (dirs: string[], realRootDir: string): Promise<boolean> {
  const realDirs = await Promise.all(dirs.map(async (dir) => tryRealpath(dir)))
  return realDirs.includes(realRootDir)
}

async function resolveConfiguredLockfileDir (
  opts: CleanOptions,
  rootDir: string,
  realRootDir: string
): Promise<string | undefined> {
  const rawSpec = await readRawLockfileDirSpec(opts, rootDir)
  if (rawSpec == null) return undefined
  const candidate = await resolveInside(rootDir, rawSpec)
  if (candidate == null) return undefined
  const realLockfileDir = await tryRealpath(candidate)
  return realLockfileDir != null && isStrictSubdir(realRootDir, realLockfileDir)
    ? realLockfileDir
    : undefined
}

async function readRawLockfileDirSpec (opts: CleanOptions, rootDir: string): Promise<string | undefined> {
  const cliLockfileDir = opts.cliOptions?.['lockfile-dir']
  if (typeof cliLockfileDir === 'string') {
    return anchorSpecToCliDir(opts.dir, cliLockfileDir)
  }
  const envLockfileDir = readEnvLockfileDir()
  if (typeof envLockfileDir === 'string') {
    return anchorSpecToCliDir(opts.dir, envLockfileDir)
  }
  const workspaceManifest = await readWorkspaceManifest(rootDir) as { lockfileDir?: unknown } | undefined
  if (typeof workspaceManifest?.lockfileDir === 'string') {
    return workspaceManifest.lockfileDir
  }
  return opts.lockfileDir
}

function readEnvLockfileDir (): string | undefined {
  let npmFallback: string | undefined
  for (const [key, value] of Object.entries(process.env)) {
    if (typeof value !== 'string') continue
    const lowerKey = key.toLowerCase()
    if (lowerKey === 'pnpm_config_lockfile_dir') return value
    if (lowerKey === 'npm_config_lockfile_dir') npmFallback = value
  }
  return npmFallback
}

async function anchorSpecToCliDir (cliDir: string, rawSpec: string): Promise<string | undefined> {
  const nativeSpec = path.sep === '\\' ? rawSpec.replaceAll('/', '\\') : rawSpec
  if (path.parse(nativeSpec).root !== '') {
    return path.isAbsolute(nativeSpec) ? nativeSpec : undefined
  }
  const realCliDir = await tryRealpath(cliDir)
  return realCliDir != null ? `${appendPathSep(realCliDir)}${nativeSpec}` : undefined
}

async function cleanProjectDir (opts: CleanProjectContext, dir: string): Promise<void> {
  const fullModulesDir = await resolveProjectModulesDir({
    modulesDir: opts.modulesDir,
    projectDir: dir,
    realRootDir: opts.realRootDir,
  })
  if (fullModulesDir != null && !opts.cleanedModulesDirs.has(fullModulesDir)) {
    opts.cleanedModulesDirs.add(fullModulesDir)
    await cleanModulesDir(opts.cwd, fullModulesDir)
  }
  if (opts.removeLockfile) {
    await removeProjectLockfile(opts, dir)
  }
}

async function resolveProjectModulesDir (
  opts: {
    modulesDir: string
    projectDir: string
    realRootDir: string
  }
): Promise<string | undefined> {
  const resolved = await resolveInside(opts.projectDir, opts.modulesDir)
  if (resolved == null || !isStrictSubdir(opts.realRootDir, resolved)) {
    return undefined
  }
  return resolved
}

const PNPM_HIDDEN_ENTRIES = new Set(['.bin', '.modules.yaml', '.pnpm', '.pnpm-workspace-state-v1.json'])

async function cleanModulesDir (cwd: string, modulesDir: string): Promise<void> {
  const stat = await lstatIfExists(modulesDir)
  if (stat == null) return
  if (stat.isSymbolicLink()) {
    printRemoving(cwd, modulesDir)
    await rimraf(modulesDir)
    return
  }
  if (!stat.isDirectory()) return
  const entries = await removableModulesEntries(modulesDir)
  if (entries.length === 0) return
  printRemoving(cwd, modulesDir)
  await Promise.all(entries.map(async (entry) => rimraf(entry)))
}

async function removableModulesEntries (modulesDir: string): Promise<string[]> {
  let items: string[]
  try {
    items = await fs.readdir(modulesDir)
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ENOENT') return []
    throw err
  }
  return items
    .filter((item) => item[0] !== '.' || PNPM_HIDDEN_ENTRIES.has(item))
    .map((item) => path.join(modulesDir, item))
}

async function removeProjectLockfile (opts: CleanProjectContext, dir: string): Promise<void> {
  const lockfilePath = await resolveInside(dir, 'pnpm-lock.yaml')
  if (lockfilePath == null || !isStrictSubdir(opts.realRootDir, lockfilePath)) return
  if (opts.removedLockfiles.has(lockfilePath)) return
  opts.removedLockfiles.add(lockfilePath)
  const stat = await lstatIfExists(lockfilePath)
  if (stat == null || (!stat.isSymbolicLink() && stat.isDirectory())) return
  printRemoving(opts.cwd, lockfilePath)
  await rimraf(lockfilePath)
}

async function removeExternalVirtualStore (
  opts: {
    cwd: string
    modulesDir: string
    rootDir: string
    virtualStoreDir: string
  }
): Promise<void> {
  const virtualStoreInProject = await resolveInside(opts.rootDir, opts.virtualStoreDir)
  if (virtualStoreInProject == null) return
  const rootModulesDir = await resolveInside(opts.rootDir, opts.modulesDir)
  if (rootModulesDir != null && isSubdir(rootModulesDir, virtualStoreInProject)) return
  if (await lstatIfExists(virtualStoreInProject) == null) return
  printRemoving(opts.cwd, virtualStoreInProject)
  await rimraf(virtualStoreInProject)
}

async function resolveInside (rootDir: string, targetSpec: string): Promise<string | undefined> {
  const realRoot = await tryRealpath(rootDir)
  if (realRoot == null) {
    return undefined
  }
  const candidatePath = normalizeTargetSpec(realRoot, targetSpec)
  if (candidatePath == null) {
    return undefined
  }
  const parsed = path.parse(candidatePath)
  if (parsed.base.replace(/[. ]+$/u, '') === '') {
    return undefined
  }
  const realParent = await resolveParentDir(realRoot, parsed)
  if (realParent == null) {
    return undefined
  }
  const resolved = path.join(realParent, parsed.base)
  return isStrictSubdir(realRoot, resolved) ? resolved : undefined
}

async function resolveParentDir (realRoot: string, parsed: ParsedPath): Promise<string | undefined> {
  if (parsed.dir === realRoot) {
    return realRoot
  }
  if (!parsed.dir.split(path.sep).some(needsStepwiseResolution)) {
    return tryRealpath(parsed.dir)
  }
  return resolveParentSegments(realRoot, parsed)
}

function needsStepwiseResolution (segment: string): boolean {
  return segment === '..' || (path.sep === '\\' && segment !== '' && segment.replace(/[. ]+$/u, '') === '')
}

async function resolveParentSegments (realRoot: string, parsed: ParsedPath): Promise<string | undefined> {
  const rootPrefix = appendPathSep(realRoot)
  const underRealRoot = parsed.dir.startsWith(rootPrefix)
  const anchor = underRealRoot ? realRoot : await tryRealpath(parsed.root)
  if (anchor == null) {
    return undefined
  }
  const tail = underRealRoot ? parsed.dir.slice(rootPrefix.length) : parsed.dir.slice(parsed.root.length)
  const segments = tail.split(path.sep).filter((segment) => segment !== '' && segment !== '.')
  return segments.reduce<Promise<string | undefined>>(
    async (currentPromise, segment) => stepParentSegment(await currentPromise, segment),
    Promise.resolve(anchor)
  )
}

async function stepParentSegment (current: string | undefined, segment: string): Promise<string | undefined> {
  if (current == null) {
    return undefined
  }
  return segment === '..' ? path.dirname(current) : tryRealpath(path.join(current, segment))
}

function normalizeTargetSpec (realRoot: string, targetSpec: string): string | undefined {
  const nativeSpec = path.sep === '\\' ? targetSpec.replaceAll('/', '\\') : targetSpec
  if (path.parse(nativeSpec).root === '') {
    return `${appendPathSep(realRoot)}${nativeSpec}`
  }
  return path.isAbsolute(nativeSpec) ? nativeSpec : undefined
}

function appendPathSep (dirPath: string): string {
  return dirPath.endsWith(path.sep) ? dirPath : `${dirPath}${path.sep}`
}

async function tryRealpath (targetPath: string): Promise<string | undefined> {
  try {
    return await fs.realpath(targetPath)
  } catch (err: unknown) {
    if (isExpectedResolutionError(err)) return undefined
    throw err
  }
}

async function lstatIfExists (targetPath: string): Promise<Stats | undefined> {
  try {
    return await fs.lstat(targetPath)
  } catch (err: unknown) {
    if (isExpectedResolutionError(err)) return undefined
    throw err
  }
}

function isExpectedResolutionError (err: unknown): boolean {
  return isError(err) &&
    'code' in err &&
    (err.code === 'ENOENT' || err.code === 'ENOTDIR' || err.code === 'ELOOP')
}

function printRemoving (cwd: string, dir: string): void {
  process.stdout.write(`Removing ${path.relative(cwd, dir) || '.'}\n`)
}

async function getProjectDirs (
  opts: CleanOptions,
  { narrowed, realRootDir, rootDir }: { narrowed: boolean, realRootDir: string, rootDir: string }
): Promise<string[]> {
  if (narrowed) {
    if (opts.selectedProjectsGraph != null) {
      return Object.keys(opts.selectedProjectsGraph)
    }
    if (opts.workspaceRoot) {
      return [rootDir]
    }
  }
  if (!opts.workspaceDir) {
    return [opts.dir]
  }
  const pkgs = await findWorkspaceProjectsNoCheck(opts.workspaceDir, {
    patterns: opts.workspacePackagePatterns,
    modulesDir: opts.modulesDir,
    modulesDirsByProjectName: opts.modulesDirsByProjectName,
  })
  const projectDirs = pkgs.map((pkg) => pkg.rootDir)
  return await dirsIncludeRoot(projectDirs, realRootDir)
    ? projectDirs
    : [opts.workspaceDir, ...projectDirs]
}
