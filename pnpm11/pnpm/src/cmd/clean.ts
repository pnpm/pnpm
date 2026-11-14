import path from 'node:path'

import { docsUrl } from '@pnpm/cli.utils'
import { anchorRawLockfileDir, types as allTypes } from '@pnpm/config.reader'
import type { ProjectsGraph } from '@pnpm/types'
import { safeReadProjectManifestOnly } from '@pnpm/workspace.project-manifest-reader'
import { findWorkspaceProjectsNoCheck } from '@pnpm/workspace.projects-reader'
import { strict as isStrictSubdir } from 'is-subdir'
import { pick } from 'ramda'
import { renderHelp } from 'render-help'

import {
  cleanModulesDir,
  invalidateWorkspaceState,
  removeExternalVirtualStore,
  removeLockfilePath,
  resolveConfiguredLockfileDir,
  resolveInside,
  resolveProjectModulesDir,
  tryRealpath,
} from './cleanRemoval.js'

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
    recursive?: boolean
  }
  dir: string
  filter?: string[]
  filterProd?: string[]
  lockfileDir?: string
  modulesDir?: string
  modulesDirsByProjectName?: Record<string, string>
  rawLockfileDir?: string
  selectedProjectsGraph?: ProjectsGraph
  sharedWorkspaceLockfile?: boolean
  virtualStoreDir?: string
  workspaceDir?: string
  workspacePackagePatterns?: string[]
  workspaceRoot?: boolean
}

export async function handler (opts: CleanOptions): Promise<void> {
  await cleanProjects({ ...opts, removeLockfile: opts.cliOptions?.lockfile === true })
}

interface CleanTarget {
  dir: string
  name?: string
}

interface CleanRunScope {
  cleansRoot: boolean
  cwd: string
  dirs: string[]
  modulesDir: string
  narrowed: boolean
  realRootDir: string
  resolvedLockfileDir: string | undefined
  rootDir: string
  sharedModulesDir: string | undefined
  targets: CleanTarget[]
}

export async function cleanProjects (opts: CleanOptions & { removeLockfile: boolean }): Promise<void> {
  const scope = await buildCleanRunScope(opts)
  if (scope == null) return
  const modulesTargets = await collectModulesTargets(opts, scope)
  await Promise.all(modulesTargets.map(async ({ dir, preserveShared }) => cleanModulesDir(scope.cwd, dir, preserveShared)))
  if (scope.sharedModulesDir != null) {
    await invalidateWorkspaceState(scope.realRootDir, scope.sharedModulesDir)
  }
  if (opts.removeLockfile) {
    const lockfileTargets = await collectLockfileTargets(scope)
    await Promise.all(lockfileTargets.map(async (lockfilePath) => removeLockfilePath(scope.cwd, lockfilePath)))
  }
  if (!scope.narrowed && opts.virtualStoreDir) {
    await removeExternalVirtualStore({
      cwd: scope.cwd,
      modulesDir: scope.modulesDir,
      rootDir: scope.rootDir,
      virtualStoreDir: opts.virtualStoreDir,
    })
  }
}

async function buildCleanRunScope (opts: CleanOptions): Promise<CleanRunScope | undefined> {
  const rootDir = opts.workspaceDir ?? opts.dir
  const realRootDir = await tryRealpath(rootDir)
  if (realRootDir == null) return undefined
  const modulesDir = opts.modulesDir ?? 'node_modules'
  const cwd = await tryRealpath(process.cwd()) ?? process.cwd()
  const narrowed = isNarrowedRun(opts)
  const targets = await getProjectTargets(opts, { narrowed, realRootDir, rootDir })
  const dirs = targets.map((target) => target.dir)
  const cleansRoot = !narrowed || await dirsIncludeRoot(dirs, realRootDir)
  const rawLockfileSpec = readRawLockfileDirSpec(opts, rootDir)
  const resolvedLockfileDir = rawLockfileSpec != null
    ? await resolveConfiguredLockfileDir(rawLockfileSpec, rootDir, realRootDir)
    : undefined
  const sharedModulesDir = await resolveSharedWorkspaceModulesDir(opts, {
    modulesDir,
    narrowed,
    rawLockfileSpec,
    realRootDir,
    resolvedLockfileDir,
    rootDir,
  })
  return {
    cleansRoot,
    cwd,
    dirs,
    modulesDir,
    narrowed,
    realRootDir,
    resolvedLockfileDir,
    rootDir,
    sharedModulesDir,
    targets,
  }
}

function isNarrowedRun (opts: Pick<CleanOptions, 'cliOptions' | 'filter' | 'filterProd' | 'workspaceDir' | 'workspaceRoot'>): boolean {
  return Boolean(
    opts.filter?.length ||
    opts.filterProd?.length ||
    opts.workspaceRoot ||
    (opts.workspaceDir && opts.cliOptions?.recursive === false)
  )
}

async function resolveSharedWorkspaceModulesDir (
  opts: CleanOptions,
  ctx: {
    modulesDir: string
    narrowed: boolean
    rawLockfileSpec: string | undefined
    realRootDir: string
    resolvedLockfileDir: string | undefined
    rootDir: string
  }
): Promise<string | undefined> {
  if (!ctx.narrowed || !opts.workspaceDir || opts.sharedWorkspaceLockfile === false) {
    return undefined
  }
  const anchorDir = await resolveSharedAnchorDir(ctx)
  if (anchorDir == null) return undefined
  return resolveProjectModulesDir({
    modulesDir: ctx.modulesDir,
    projectDir: anchorDir,
    realRootDir: ctx.realRootDir,
  })
}

async function resolveSharedAnchorDir (
  ctx: {
    rawLockfileSpec: string | undefined
    realRootDir: string
    resolvedLockfileDir: string | undefined
    rootDir: string
  }
): Promise<string | undefined> {
  if (ctx.rawLockfileSpec == null) return ctx.rootDir
  if (ctx.resolvedLockfileDir != null) return ctx.resolvedLockfileDir
  const candidate = await tryRealpath(path.resolve(ctx.rootDir, ctx.rawLockfileSpec))
  return candidate === ctx.realRootDir ? ctx.rootDir : undefined
}

async function collectModulesTargets (
  opts: CleanOptions,
  scope: CleanRunScope
): Promise<Array<{ dir: string, preserveShared: boolean }>> {
  const candidateModules = await Promise.all(scope.targets.map(async (target) => resolveProjectModulesDir({
    modulesDir: resolveModulesLeafForProject(opts, target.name, scope.modulesDir),
    projectDir: target.dir,
    realRootDir: scope.realRootDir,
  })))
  if (!scope.narrowed && scope.cleansRoot && scope.resolvedLockfileDir != null) {
    candidateModules.push(await resolveProjectModulesDir({
      modulesDir: scope.modulesDir,
      projectDir: scope.resolvedLockfileDir,
      realRootDir: scope.realRootDir,
    }))
  }
  const seen = new Set<string>()
  const unique: Array<{ dir: string, preserveShared: boolean }> = []
  for (const resolved of candidateModules) {
    if (resolved != null && !seen.has(resolved)) {
      seen.add(resolved)
      unique.push({ dir: resolved, preserveShared: resolved === scope.sharedModulesDir })
    }
  }
  return unique
}

function hasPerProjectModulesDirs (opts: CleanOptions): boolean {
  return opts.lockfileDir == null &&
    opts.modulesDirsByProjectName != null &&
    Object.keys(opts.modulesDirsByProjectName).length > 0
}

function resolveModulesLeafForProject (opts: CleanOptions, projectName: string | undefined, defaultModulesDir: string): string {
  if (hasPerProjectModulesDirs(opts) && projectName != null) {
    const custom = opts.modulesDirsByProjectName?.[projectName]
    if (custom != null) return custom
  }
  return defaultModulesDir
}

async function collectLockfileTargets (scope: CleanRunScope): Promise<string[]> {
  const candidateDirs = [...scope.dirs]
  if (scope.cleansRoot) {
    candidateDirs.push(scope.rootDir)
    if (scope.resolvedLockfileDir != null) {
      candidateDirs.push(scope.resolvedLockfileDir)
    }
  }
  const resolvedPaths = await Promise.all(candidateDirs.map(async (dir) => {
    const lockfilePath = await resolveInside(dir, 'pnpm-lock.yaml')
    return lockfilePath != null && isStrictSubdir(scope.realRootDir, lockfilePath) ? lockfilePath : undefined
  }))
  return Array.from(new Set(resolvedPaths.filter((item): item is string => item != null)))
}

async function dirsIncludeRoot (dirs: string[], realRootDir: string): Promise<boolean> {
  const realDirs = await Promise.all(dirs.map(async (dir) => tryRealpath(dir)))
  return realDirs.includes(realRootDir)
}

function readRawLockfileDirSpec (opts: CleanOptions, rootDir: string): string | undefined {
  if (typeof opts.rawLockfileDir === 'string') {
    return opts.rawLockfileDir
  }
  const cliLockfileDir = opts.cliOptions?.['lockfile-dir']
  if (typeof cliLockfileDir === 'string') {
    return anchorRawLockfileDir(opts.dir, cliLockfileDir)
  }
  if (opts.lockfileDir != null && opts.lockfileDir !== rootDir) {
    return opts.lockfileDir
  }
  return undefined
}

async function getProjectTargets (
  opts: CleanOptions,
  { narrowed, realRootDir, rootDir }: { narrowed: boolean, realRootDir: string, rootDir: string }
): Promise<CleanTarget[]> {
  if (!opts.workspaceDir) {
    return opts.selectedProjectsGraph != null
      ? targetsFromGraph(opts.selectedProjectsGraph)
      : [{ dir: opts.dir, name: await readTargetNameIfNeeded(opts, opts.dir) }]
  }
  if (narrowed) {
    return getNarrowedWorkspaceTargets(opts, rootDir)
  }
  return getWholeWorkspaceTargets(opts, opts.workspaceDir, realRootDir)
}

async function getNarrowedWorkspaceTargets (opts: CleanOptions, rootDir: string): Promise<CleanTarget[]> {
  if (opts.selectedProjectsGraph != null) {
    return targetsFromGraph(opts.selectedProjectsGraph)
  }
  const singleDir = resolveSingleNarrowedDir(opts, rootDir)
  return singleDir != null
    ? [{ dir: singleDir, name: await readTargetNameIfNeeded(opts, singleDir) }]
    : []
}

function resolveSingleNarrowedDir (opts: CleanOptions, rootDir: string): string | undefined {
  if (opts.workspaceRoot) return rootDir
  return opts.cliOptions?.recursive === false ? opts.dir : undefined
}

async function getWholeWorkspaceTargets (
  opts: CleanOptions,
  workspaceDir: string,
  realRootDir: string
): Promise<CleanTarget[]> {
  const pkgs = await findWorkspaceProjectsNoCheck(workspaceDir, {
    patterns: opts.workspacePackagePatterns,
    modulesDir: opts.modulesDir,
    modulesDirsByProjectName: opts.modulesDirsByProjectName,
  })
  const projectTargets: CleanTarget[] = pkgs.map((pkg) => ({ dir: pkg.rootDir, name: pkg.manifest.name }))
  if (await dirsIncludeRoot(projectTargets.map((target) => target.dir), realRootDir)) {
    return projectTargets
  }
  return [
    { dir: workspaceDir, name: await readTargetNameIfNeeded(opts, workspaceDir) },
    ...projectTargets,
  ]
}

function targetsFromGraph (graph: ProjectsGraph): CleanTarget[] {
  return Object.entries(graph).map(([dir, node]) => ({ dir, name: node.package.manifest.name }))
}

async function readTargetNameIfNeeded (opts: CleanOptions, dir: string): Promise<string | undefined> {
  if (!hasPerProjectModulesDirs(opts)) {
    return undefined
  }
  const manifest = await safeReadProjectManifestOnly(dir)
  return manifest?.name
}
