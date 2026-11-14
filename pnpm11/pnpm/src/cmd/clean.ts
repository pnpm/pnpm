import { docsUrl } from '@pnpm/cli.utils'
import { anchorRawLockfileDir, types as allTypes } from '@pnpm/config.reader'
import type { ProjectsGraph } from '@pnpm/types'
import { safeReadProjectManifestOnly } from '@pnpm/workspace.project-manifest-reader'
import { strict as isStrictSubdir } from 'is-subdir'
import { pick } from 'ramda'
import { renderHelp } from 'render-help'

import { discoverWorkspacePackagesForClean } from './cleanDiscovery.js'
import {
  cleanModulesDir,
  collectNonRootRealDirs,
  dirsIncludeRoot,
  invalidateWorkspaceState,
  isCleanableModulesDir,
  type ModulesCleanMode,
  removeExternalVirtualStore,
  removeLockfilePath,
  resolveConfiguredLockfileDir,
  resolveInside,
  resolveProjectModulesDir,
  resolveSharedAnchorDir,
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
  protectedMemberDirs: string[]
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
  await Promise.all(modulesTargets.map(async ({ dir, mode }) => cleanModulesDir(scope.cwd, dir, mode)))
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
  const narrowed = isNarrowedRun(opts)
  const targetGroup = await getProjectTargets(opts, { modulesDir, narrowed, realRootDir, rootDir })
  const dirs = targetGroup.targets.map((target) => target.dir)
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
    cleansRoot: !narrowed || await dirsIncludeRoot(dirs, realRootDir),
    cwd: await tryRealpath(process.cwd()) ?? process.cwd(),
    dirs,
    modulesDir,
    narrowed,
    protectedMemberDirs: targetGroup.protectedMemberDirs,
    realRootDir,
    resolvedLockfileDir,
    rootDir,
    sharedModulesDir,
    targets: targetGroup.targets,
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

async function collectModulesTargets (
  opts: CleanOptions,
  scope: CleanRunScope
): Promise<Array<{ dir: string, mode: ModulesCleanMode }>> {
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
  const unique: Array<{ dir: string, mode: ModulesCleanMode }> = []
  for (const resolved of candidateModules) {
    if (isCleanableModulesDir(resolved, seen, scope.protectedMemberDirs)) {
      seen.add(resolved)
      unique.push({
        dir: resolved,
        mode: resolved === scope.sharedModulesDir ? 'preserve-shared-virtual-store' : 'all',
      })
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
  { modulesDir, narrowed, realRootDir, rootDir }: {
    modulesDir: string
    narrowed: boolean
    realRootDir: string
    rootDir: string
  }
): Promise<{ targets: CleanTarget[], protectedMemberDirs: string[] }> {
  if (!opts.workspaceDir) {
    const targets = opts.selectedProjectsGraph != null
      ? targetsFromGraph(opts.selectedProjectsGraph)
      : [{ dir: opts.dir, name: await readTargetNameIfNeeded(opts, opts.dir) }]
    return { targets, protectedMemberDirs: [] }
  }
  if (narrowed) {
    const targets = await getNarrowedWorkspaceTargets(opts, rootDir)
    const protectedMemberDirs = modulesDir !== 'node_modules' || hasPerProjectModulesDirs(opts)
      ? await readProtectedMemberDirs(opts, opts.workspaceDir, realRootDir)
      : []
    return { targets, protectedMemberDirs }
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
): Promise<{ targets: CleanTarget[], protectedMemberDirs: string[] }> {
  const pkgs = await discoverWorkspacePackagesForClean(workspaceDir, {
    hasPerProjectModulesDirs: hasPerProjectModulesDirs(opts),
    modulesDir: opts.modulesDir,
    modulesDirsByProjectName: opts.modulesDirsByProjectName,
    patterns: opts.workspacePackagePatterns,
  })
  const projectTargets: CleanTarget[] = pkgs.map((pkg) => ({ dir: pkg.rootDir, name: pkg.manifest.name }))
  const realMemberDirs = await collectNonRootRealDirs(projectTargets.map((target) => target.dir), realRootDir)
  if (await dirsIncludeRoot(projectTargets.map((target) => target.dir), realRootDir)) {
    return { targets: projectTargets, protectedMemberDirs: realMemberDirs }
  }
  return {
    targets: [
      { dir: workspaceDir, name: await readTargetNameIfNeeded(opts, workspaceDir) },
      ...projectTargets,
    ],
    protectedMemberDirs: realMemberDirs,
  }
}

async function readProtectedMemberDirs (
  opts: CleanOptions,
  workspaceDir: string,
  realRootDir: string
): Promise<string[]> {
  const pkgs = await discoverWorkspacePackagesForClean(workspaceDir, {
    hasPerProjectModulesDirs: hasPerProjectModulesDirs(opts),
    modulesDir: opts.modulesDir,
    modulesDirsByProjectName: opts.modulesDirsByProjectName,
    patterns: opts.workspacePackagePatterns,
  })
  return collectNonRootRealDirs(pkgs.map((pkg) => pkg.rootDir), realRootDir)
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
