import path from 'node:path'

import { docsUrl } from '@pnpm/cli.utils'
import { anchorRawLockfileDir, types as allTypes } from '@pnpm/config.reader'
import type { ProjectsGraph } from '@pnpm/types'
import { safeReadProjectManifestOnly } from '@pnpm/workspace.project-manifest-reader'
import { pick } from 'ramda'
import { renderHelp } from 'render-help'

import { collectExplicitLiteralMemberDirs, discoverWorkspacePackagesForClean } from './cleanDiscovery.js'
import { collectLockfileTargets, filterCleanableTargets, planRemovesInstallState } from './cleanPlan.js'
import {
  cleanModulesDir,
  collectNonRootRealDirs,
  dirsIncludeRoot,
  invalidateWorkspaceState,
  type PlannedModulesTarget,
  removeExternalVirtualStore,
  removeLockfilePath,
  resolveConfiguredLockfileDir,
  resolveExternalVirtualStoreTarget,
  resolvePreservedVirtualStoreEntry,
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
  sharedAnchorDir: string | undefined
  sharedModulesDir: string | undefined
  targets: CleanTarget[]
}

export interface CiCleanPlan {
  cleansModulesDir: (anchorDir: string, modulesDir?: string) => Promise<boolean>
  execute: () => Promise<void>
}

export async function cleanProjects (opts: CleanOptions & { removeLockfile: boolean }): Promise<void> {
  const scope = await buildCleanRunScope(opts)
  if (scope == null) return
  const [modulesTargets, externalVirtualStoreDir] = await Promise.all([
    collectModulesTargets(opts, scope),
    resolvePlannedExternalVirtualStore(opts, scope),
  ])
  await executePlannedClean({ externalVirtualStoreDir, modulesTargets, removeLockfile: opts.removeLockfile, scope })
}

export async function prepareCiCleanPlan (opts: CleanOptions): Promise<CiCleanPlan> {
  const scope = await buildCleanRunScope(opts)
  if (scope == null) {
    return {
      cleansModulesDir: async () => false,
      execute: async () => {},
    }
  }
  const [modulesTargets, externalVirtualStoreDir] = await Promise.all([
    collectModulesTargets(opts, scope),
    resolvePlannedExternalVirtualStore(opts, scope),
  ])
  return {
    cleansModulesDir: async (anchorDir: string, modulesDir = 'node_modules') => planRemovesInstallState({
      externalVirtualStoreDir,
      installStateDir: path.resolve(anchorDir, modulesDir, '.pnpm'),
      modulesTargets,
      realRootDir: scope.realRootDir,
    }),
    execute: async () => executePlannedClean({ externalVirtualStoreDir, modulesTargets, removeLockfile: false, scope }),
  }
}

async function resolvePlannedExternalVirtualStore (opts: CleanOptions, scope: CleanRunScope): Promise<string | undefined> {
  if (scope.narrowed || !opts.virtualStoreDir) return undefined
  return resolveExternalVirtualStoreTarget({
    modulesDir: scope.modulesDir,
    protectedMemberDirs: scope.protectedMemberDirs,
    rootDir: scope.rootDir,
    virtualStoreDir: opts.virtualStoreDir,
  })
}

async function executePlannedClean (
  { externalVirtualStoreDir, modulesTargets, removeLockfile, scope }: {
    externalVirtualStoreDir: string | undefined
    modulesTargets: PlannedModulesTarget[]
    removeLockfile: boolean
    scope: CleanRunScope
  }
): Promise<void> {
  await Promise.all(modulesTargets.map(async ({ dir, mode }) => cleanModulesDir(scope.cwd, dir, mode)))
  if (scope.sharedModulesDir != null) {
    await invalidateWorkspaceState(scope.realRootDir, scope.sharedModulesDir)
  }
  if (removeLockfile) {
    const lockfileTargets = await collectLockfileTargets(scope)
    await Promise.all(lockfileTargets.map(async (lockfilePath) => removeLockfilePath(scope.cwd, lockfilePath)))
  }
  await removeExternalVirtualStore({ cwd: scope.cwd, externalVirtualStoreDir })
}

async function buildCleanRunScope (opts: CleanOptions): Promise<CleanRunScope | undefined> {
  const rootDir = opts.workspaceDir ?? opts.dir
  const realRootDir = await tryRealpath(rootDir)
  if (realRootDir == null) return undefined
  const modulesDir = opts.modulesDir ?? 'node_modules'
  const narrowed = isNarrowedRun(opts)
  const targetGroup = await getProjectTargets(opts, { narrowed, realRootDir, rootDir })
  const dirs = targetGroup.targets.map((target) => target.dir)
  const rawLockfileSpec = readRawLockfileDirSpec(opts, rootDir)
  const resolvedLockfileDir = rawLockfileSpec != null
    ? await resolveConfiguredLockfileDir(rawLockfileSpec, rootDir, realRootDir)
    : undefined
  const shared = await resolveSharedWorkspaceModules(opts, {
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
    sharedAnchorDir: shared.anchorDir,
    sharedModulesDir: shared.modulesDir,
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

async function resolveSharedWorkspaceModules (
  opts: CleanOptions,
  ctx: {
    modulesDir: string
    narrowed: boolean
    rawLockfileSpec: string | undefined
    realRootDir: string
    resolvedLockfileDir: string | undefined
    rootDir: string
  }
): Promise<{ anchorDir?: string, modulesDir?: string }> {
  if (!ctx.narrowed || !opts.workspaceDir || opts.sharedWorkspaceLockfile === false) {
    return {}
  }
  const anchorDir = await resolveSharedAnchorDir(ctx)
  if (anchorDir == null) return {}
  const modulesDir = await resolveProjectModulesDir({
    modulesDir: ctx.modulesDir,
    projectDir: anchorDir,
    realRootDir: ctx.realRootDir,
  })
  return { anchorDir, modulesDir }
}

async function collectModulesTargets (
  opts: CleanOptions,
  scope: CleanRunScope
): Promise<PlannedModulesTarget[]> {
  const preservedEntry = await resolvePreservedVirtualStoreEntry({
    anchorDir: scope.sharedAnchorDir,
    realRootDir: scope.realRootDir,
    sharedModulesDir: scope.sharedModulesDir,
    virtualStoreDir: opts.virtualStoreDir,
  })
  const candidateModules = await Promise.all(scope.targets.map(async (target) => {
    const [resolved, ownerDir] = await Promise.all([
      resolveProjectModulesDir({
        modulesDir: resolveModulesLeafForProject(opts, target.name, scope.modulesDir),
        projectDir: target.dir,
        realRootDir: scope.realRootDir,
      }),
      tryRealpath(target.dir),
    ])
    return { ownerDir, resolved }
  }))
  if (!scope.narrowed && scope.cleansRoot && scope.resolvedLockfileDir != null) {
    candidateModules.push({
      ownerDir: undefined,
      resolved: await resolveProjectModulesDir({
        modulesDir: scope.modulesDir,
        projectDir: scope.resolvedLockfileDir,
        realRootDir: scope.realRootDir,
      }),
    })
  }
  return filterCleanableTargets(candidateModules, {
    preservedEntry,
    protectedMemberDirs: scope.protectedMemberDirs,
    sharedModulesDir: scope.sharedModulesDir,
  })
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
  { narrowed, realRootDir, rootDir }: {
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
    const protectedMemberDirs = hasCustomCleanDirs(opts)
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
  const explicitDirs = hasCustomCleanDirs(opts)
    ? await collectExplicitLiteralMemberDirs(workspaceDir, opts.workspacePackagePatterns)
    : []
  const realMemberDirs = await collectNonRootRealDirs(
    [...projectTargets.map((target) => target.dir), ...explicitDirs],
    realRootDir
  )
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
  const [pkgs, explicitDirs] = await Promise.all([
    discoverWorkspacePackagesForClean(workspaceDir, {
      hasPerProjectModulesDirs: hasPerProjectModulesDirs(opts),
      modulesDir: opts.modulesDir,
      modulesDirsByProjectName: opts.modulesDirsByProjectName,
      patterns: opts.workspacePackagePatterns,
    }),
    collectExplicitLiteralMemberDirs(workspaceDir, opts.workspacePackagePatterns),
  ])
  return collectNonRootRealDirs([...pkgs.map((pkg) => pkg.rootDir), ...explicitDirs], realRootDir)
}

function hasCustomCleanDirs (opts: CleanOptions): boolean {
  return (opts.modulesDir != null && opts.modulesDir !== 'node_modules') ||
    hasPerProjectModulesDirs(opts) ||
    Boolean(opts.virtualStoreDir) ||
    readRawLockfileDirSpec(opts, opts.workspaceDir ?? opts.dir) != null
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
