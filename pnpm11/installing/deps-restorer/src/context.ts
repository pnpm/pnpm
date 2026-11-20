import path from 'node:path'

import { installabilityUnderForce } from '@pnpm/config.package-is-installable'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import type { DependenciesGraphNode, LockfileToDepGraphResult } from '@pnpm/deps.graph-builder'
import type { DepsStateCache } from '@pnpm/deps.graph-hasher'
import { PnpmError } from '@pnpm/error'
import type { filterLockfileByEngine } from '@pnpm/lockfile.filtering'
import { type LockfileObject, readCurrentLockfile, readWantedLockfile } from '@pnpm/lockfile.fs'
import { findLockedRootNodeRuntime } from '@pnpm/lockfile.utils'
import type { AllowBuild, DepPath, HoistedDependencies, ProjectId, RegistriesByScope } from '@pnpm/types'
import { pathAbsolute } from 'path-absolute'
import { pick } from 'ramda'
import { realpathMissing } from 'realpath-missing'

import type { HeadlessOptions, Project } from './types.js'

export type LockfileFilterOptions = Parameters<typeof filterLockfileByEngine>[1] & {
  registriesByScope: RegistriesByScope
}

/** The directories, lockfiles, and projects a headless install works with. */
export interface HeadlessContext {
  opts: HeadlessOptions
  lockfileDir: string
  wantedLockfile: LockfileObject
  currentLockfile: LockfileObject | null
  depsStateCache: DepsStateCache
  rootModulesDir: string
  virtualStoreDir: string
  hoistedModulesDir: string
  publicHoistedModulesDir: string
  selectedProjects: Project[]
  /**
   * The same array as `selectedProjects` when `projectDirsRunningScripts` is
   * not set, so it also holds the projects added to the selection later.
   */
  projectsRunningScripts: Project[]
  /** The directories of `projectsRunningScripts` at the time of selection. */
  projectDirsRunningScripts: Set<string>
  skipped: Set<DepPath>
  rootRuntimeNodeVersion?: string
  currentEngine: HeadlessOptions['currentEngine']
  filterOpts: LockfileFilterOptions
  skipPostImportLinking: boolean
}

/** The dependency graph of a headless install and the selection it was built from. */
export interface HeadlessDepGraph extends LockfileToDepGraphResult {
  added: number
  allowBuild: AllowBuild | undefined
  depNodes: DependenciesGraphNode[]
  filteredLockfile: LockfileObject
  importerIds: ProjectId[]
  includeUnchangedDeps: boolean
}

/** What linking the dependencies of a headless install produced. */
export interface LinkedDependencies {
  /** Nested `.bin` directories whose bins wait for the dependency builds. */
  heldBackBinsDirs: string[]
  linkedToRoot: number
  /** Unset when neither the modules directory nor the global virtual store is written. */
  newHoistedDependencies?: HoistedDependencies
}

export async function createHeadlessContext (opts: HeadlessOptions): Promise<HeadlessContext> {
  const lockfileDir = opts.lockfileDir
  const wantedLockfile = await readWantedLockfileOrThrow(opts)
  // `modulesDir` is conventionally a path relative to `lockfileDir`, but
  // some callers pass it as an absolute path. Resolve via `pathAbsolute`
  // so both forms work — `path.join` on Windows would otherwise produce a
  // doubled prefix when the second argument is also absolute.
  const modulesDir = opts.modulesDir ?? 'node_modules'
  const rootModulesDir = await realpathMissing(pathAbsolute(modulesDir, lockfileDir))
  const installStateDir = path.join(rootModulesDir, '.pnpm')
  const currentLockfile = opts.currentLockfile ?? await readCurrentLockfile(installStateDir, { ignoreIncompatible: false })
  const virtualStoreDir = pathAbsolute(opts.virtualStoreDir ?? path.join(modulesDir, '.pnpm'), lockfileDir)
  const projects = selectProjects(opts)
  assertVirtualStoreOnlyIsSupported(opts)
  const skipped = opts.skipped || new Set<DepPath>()
  const rootRuntimeNodeVersion = findLockedRootNodeRuntime(wantedLockfile)?.version
  const currentEngine = resolveCurrentEngine(opts, rootRuntimeNodeVersion)
  return {
    opts,
    lockfileDir,
    wantedLockfile,
    currentLockfile,
    depsStateCache: {},
    rootModulesDir,
    virtualStoreDir,
    hoistedModulesDir: path.join(opts.enableGlobalVirtualStore ? installStateDir : virtualStoreDir, 'node_modules'),
    publicHoistedModulesDir: rootModulesDir,
    ...projects,
    skipped,
    rootRuntimeNodeVersion,
    currentEngine,
    filterOpts: createFilterOptions(opts, { currentEngine, skipped }),
    skipPostImportLinking: opts.virtualStoreOnly === true,
  }
}

async function readWantedLockfileOrThrow (opts: HeadlessOptions): Promise<LockfileObject> {
  const wantedLockfile = opts.wantedLockfile ?? await readWantedLockfile(opts.lockfileDir, {
    ignoreIncompatible: false,
    useGitBranchLockfile: opts.useGitBranchLockfile,
    // mergeGitBranchLockfiles is intentionally not supported in headless
    mergeGitBranchLockfiles: false,
  })

  if (wantedLockfile == null) {
    throw new Error(`Headless installation requires a ${WANTED_LOCKFILE} file`)
  }
  return wantedLockfile
}

function selectProjects (opts: HeadlessOptions): Pick<HeadlessContext, 'selectedProjects' | 'projectsRunningScripts' | 'projectDirsRunningScripts'> {
  const selectedProjects = Object.values(pick(opts.selectedProjectDirs, opts.allProjects))
  const projectsRunningScripts = opts.projectDirsRunningScripts == null
    ? selectedProjects
    : Object.values(pick(opts.projectDirsRunningScripts, opts.allProjects))
  return {
    selectedProjects,
    projectsRunningScripts,
    projectDirsRunningScripts: new Set(projectsRunningScripts.map(({ rootDir }) => rootDir)),
  }
}

function assertVirtualStoreOnlyIsSupported (opts: HeadlessOptions): void {
  if (opts.virtualStoreOnly && opts.enableModulesDir === false && !opts.enableGlobalVirtualStore) {
    throw new PnpmError('CONFIG_CONFLICT_VIRTUAL_STORE_ONLY_WITH_NO_MODULES_DIR',
      'Cannot use virtualStoreOnly when enableModulesDir is false (the standard virtual store requires node_modules/.pnpm)')
  }
}

function resolveCurrentEngine (opts: HeadlessOptions, rootRuntimeNodeVersion: string | undefined): HeadlessOptions['currentEngine'] {
  const nodeVersionIsConfigured = opts.currentEngine.nodeVersion != null && opts.nodeVersionFromEnginesRuntime !== true
  if (nodeVersionIsConfigured) return opts.currentEngine
  return {
    ...opts.currentEngine,
    nodeVersion: rootRuntimeNodeVersion ?? opts.currentEngine.nodeVersion,
  }
}

function createFilterOptions (
  opts: HeadlessOptions,
  { currentEngine, skipped }: Pick<HeadlessContext, 'currentEngine' | 'skipped'>
): LockfileFilterOptions {
  return {
    include: opts.include,
    registriesByScope: opts.registriesByScope,
    resolvePeersFromWorkspaceRoot: opts.resolvePeersFromWorkspaceRoot,
    skipped,
    skipRuntimes: opts.skipRuntimes,
    currentEngine,
    ...installabilityUnderForce(opts),
    failOnMissingDependencies: true,
    lockfileDir: opts.lockfileDir,
    supportedArchitectures: opts.supportedArchitectures,
  }
}
