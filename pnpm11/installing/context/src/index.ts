import { promises as fs } from 'node:fs'
import path from 'node:path'

import { contextLogger, packageManifestLogger } from '@pnpm/core-loggers'
import type {
  IncludedDependencies,
  Modules,
} from '@pnpm/installing.modules-yaml'
import { readProjectsContext } from '@pnpm/installing.read-projects-context'
import type { LockfileObject, PatchedDepPathsStatus } from '@pnpm/lockfile.fs'
import type { WorkspacePackages } from '@pnpm/resolving.resolver-base'
import { registerProject } from '@pnpm/store.controller'
import type {
  DependencyManifest,
  DepPath,
  HoistedDependencies,
  ProjectId,
  ProjectManifest,
  ProjectRootDir,
  ProjectRootDirRealPath,
  ReadPackageHook,
  RegistriesByScope,
} from '@pnpm/types'
import { pathAbsolute } from 'path-absolute'
import { clone } from 'ramda'
import { realpathMissing } from 'realpath-missing'

import { readLockfiles } from './readLockfiles.js'

/**
 * Note that some fields are affected by modules directory state. Such fields should be used for
 * mutating the modules directory only or in a manner that does not influence dependency resolution.
 */
export interface PnpmContext {
  currentLockfile: LockfileObject
  currentLockfileIsUpToDate: boolean
  existsCurrentLockfile: boolean
  existsWantedLockfile: boolean
  existsNonEmptyWantedLockfile: boolean
  extraBinPaths: string[]
  /** Affected by existing modules directory, if it exists. */
  extraNodePaths: string[]
  lockfileHadConflicts: boolean
  /** Whether the lockfile's `(patch_hash=...)` dependency paths agree with its `patchedDependencies`. */
  patchedDepPathsStatus: PatchedDepPathsStatus
  hoistedDependencies: HoistedDependencies
  /** Required included dependencies or dependencies currently included by the modules directory. */
  include: IncludedDependencies
  modulesFile: Modules | null
  pendingBuilds: string[]
  projects: Record<string, {
    modulesDir: string
    id: ProjectId
  } & HookOptions & Required<ProjectOptions>>
  rootModulesDir: string
  hoistPattern: string[] | undefined
  /** As applied to existing modules directory, if it exists. */
  currentHoistPattern: string[] | undefined
  hoistedModulesDir: string
  publicHoistPattern: string[] | undefined
  /** As applied to existing modules directory, if it exists. */
  currentPublicHoistPattern: string[] | undefined
  lockfileDir: string
  virtualStoreDir: string
  /** As applied to existing modules directory, otherwise options. */
  virtualStoreDirMaxLength: number
  /** As applied to existing modules directory, if it exists. */
  skipped: Set<DepPath>
  storeDir: string
  wantedLockfile: LockfileObject
  wantedLockfileIsModified: boolean
  workspacePackages: WorkspacePackages
  registriesByScope: RegistriesByScope
}

export interface ProjectOptions {
  buildIndex: number
  binsDir?: string
  manifest: ProjectManifest
  modulesDir?: string
  rootDir: ProjectRootDir
  rootDirRealPath?: ProjectRootDirRealPath
}

interface HookOptions {
  originalManifest?: ProjectManifest
}

export interface GetContextOptions {
  autoInstallPeers: boolean
  ci?: boolean
  excludeLinksFromLockfile: boolean
  peersSuffixMaxLength: number
  allProjects: Array<ProjectOptions & HookOptions>
  confirmModulesPurge?: boolean
  force: boolean
  frozenLockfile?: boolean
  frozenStore?: boolean
  enableGlobalVirtualStore?: boolean
  extraBinPaths: string[]
  extendNodePath?: boolean
  lockfileDir: string
  modulesDir?: string
  nodeLinker: 'isolated' | 'hoisted' | 'pnp'
  readPackageHook?: ReadPackageHook
  include?: IncludedDependencies
  registriesByScope: RegistriesByScope
  storeDir: string
  useLockfile: boolean
  useGitBranchLockfile?: boolean
  mergeGitBranchLockfiles?: boolean
  virtualStoreDir?: string
  virtualStoreDirMaxLength: number
  workspacePackages?: WorkspacePackages

  hoistPattern?: string[] | undefined

  publicHoistPattern?: string[] | undefined
  global?: boolean
}

export async function getContext (
  opts: GetContextOptions
): Promise<PnpmContext> {
  const modulesDir = opts.modulesDir ?? 'node_modules'
  const importersContext = await readProjectsContext(opts.allProjects, { lockfileDir: opts.lockfileDir, modulesDir })
  const virtualStoreDir = await resolveVirtualStoreDir(opts.virtualStoreDir, opts.lockfileDir, importersContext.rootModulesDir)

  await prepareStoreDir(opts.storeDir, opts.lockfileDir, opts.frozenStore)
  logManifests(opts.allProjects)
  await applyReadPackageHook(importersContext.projects, opts.readPackageHook)

  const installStateDir = path.join(importersContext.rootModulesDir, '.pnpm')
  const hoistedDirs = resolveHoistedDirs({
    enableGlobalVirtualStore: opts.enableGlobalVirtualStore,
    installStateDir,
    virtualStoreDir,
    hoistPattern: opts.hoistPattern,
    extraBinPaths: opts.extraBinPaths,
  })

  const lockfiles = await readLockfiles({
    autoInstallPeers: opts.autoInstallPeers,
    ci: opts.ci,
    excludeLinksFromLockfile: opts.excludeLinksFromLockfile,
    peersSuffixMaxLength: opts.peersSuffixMaxLength,
    force: opts.force,
    frozenLockfile: opts.frozenLockfile === true,
    lockfileDir: opts.lockfileDir,
    projects: importersContext.projects,
    registry: opts.registriesByScope.default,
    useLockfile: opts.useLockfile,
    useGitBranchLockfile: opts.useGitBranchLockfile,
    mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
    installStateDir,
  })

  const ctx = buildPnpmContext({ opts, importersContext, hoistedDirs, virtualStoreDir, lockfiles })

  contextLogger.debug({
    currentLockfileExists: ctx.existsCurrentLockfile,
    storeDir: opts.storeDir,
    virtualStoreDir,
  })
  return ctx
}

function buildPnpmContext (params: {
  opts: GetContextOptions
  importersContext: Awaited<ReturnType<typeof readProjectsContext<ProjectOptions & HookOptions>>>
  hoistedDirs: { extraBinPaths: string[], hoistedModulesDir: string }
  virtualStoreDir: string
  lockfiles: Awaited<ReturnType<typeof readLockfiles>>
}): PnpmContext {
  const { opts, importersContext, hoistedDirs, virtualStoreDir, lockfiles } = params
  return {
    extraBinPaths: hoistedDirs.extraBinPaths,
    extraNodePaths: getExtraNodePaths({
      extendNodePath: opts.extendNodePath,
      nodeLinker: opts.nodeLinker,
      hoistPattern: importersContext.currentHoistPattern ?? opts.hoistPattern,
      hoistedModulesDir: hoistedDirs.hoistedModulesDir,
    }),
    hoistedDependencies: importersContext.hoistedDependencies,
    hoistedModulesDir: hoistedDirs.hoistedModulesDir,
    hoistPattern: opts.hoistPattern,
    currentHoistPattern: importersContext.currentHoistPattern,
    include: opts.include ?? importersContext.include,
    lockfileDir: opts.lockfileDir,
    modulesFile: importersContext.modules,
    pendingBuilds: importersContext.pendingBuilds,
    projects: Object.fromEntries(importersContext.projects.map((project) => [project.rootDir, project])),
    publicHoistPattern: opts.publicHoistPattern,
    currentPublicHoistPattern: importersContext.currentPublicHoistPattern,
    registriesByScope: opts.registriesByScope,
    rootModulesDir: importersContext.rootModulesDir,
    skipped: importersContext.skipped,
    storeDir: opts.storeDir,
    virtualStoreDir,
    virtualStoreDirMaxLength: importersContext.virtualStoreDirMaxLength ?? opts.virtualStoreDirMaxLength,
    workspacePackages: opts.workspacePackages ?? arrayOfWorkspacePackagesToMap(opts.allProjects),
    ...lockfiles,
  }
}

export interface PnpmSingleContext {
  currentLockfile: LockfileObject
  currentLockfileIsUpToDate: boolean
  existsCurrentLockfile: boolean
  existsWantedLockfile: boolean
  existsNonEmptyWantedLockfile: boolean
  /** Affected by existing modules directory, if it exists. */
  extraBinPaths: string[]
  extraNodePaths: string[]
  lockfileHadConflicts: boolean
  /** Whether the lockfile's `(patch_hash=...)` dependency paths agree with its `patchedDependencies`. */
  patchedDepPathsStatus: PatchedDepPathsStatus
  hoistedDependencies: HoistedDependencies
  hoistedModulesDir: string
  hoistPattern: string[] | undefined
  manifest: ProjectManifest
  modulesDir: string
  importerId: string
  prefix: string
  /** Required included dependencies or dependencies currently included by the modules directory. */
  include: IncludedDependencies
  modulesFile: Modules | null
  pendingBuilds: string[]
  publicHoistPattern: string[] | undefined
  registriesByScope: RegistriesByScope
  rootModulesDir: string
  lockfileDir: string
  virtualStoreDir: string
  /** As applied to existing modules directory, if it exists. */
  skipped: Set<string>
  storeDir: string
  wantedLockfile: LockfileObject
  wantedLockfileIsModified: boolean
}

export interface GetContextForSingleImporterOptions {
  autoInstallPeers: boolean
  ci?: boolean
  enableGlobalVirtualStore?: boolean
  excludeLinksFromLockfile: boolean
  peersSuffixMaxLength: number
  force: boolean
  frozenStore?: boolean
  confirmModulesPurge?: boolean
  extraBinPaths: string[]
  extendNodePath?: boolean
  lockfileDir: string
  nodeLinker: 'isolated' | 'hoisted' | 'pnp'
  modulesDir?: string
  readPackageHook?: ReadPackageHook
  include?: IncludedDependencies
  dir: string
  registriesByScope: RegistriesByScope
  storeDir: string
  useLockfile: boolean
  useGitBranchLockfile?: boolean
  mergeGitBranchLockfiles?: boolean
  virtualStoreDir?: string
  virtualStoreDirMaxLength: number
  hoistPattern?: string[] | undefined
  publicHoistPattern?: string[] | undefined
}

export async function getContextForSingleImporter (
  manifest: ProjectManifest,
  opts: GetContextForSingleImporterOptions
): Promise<PnpmSingleContext> {
  const importerContext = await readProjectsContext(
    [{ rootDir: opts.dir as ProjectRootDir }],
    { lockfileDir: opts.lockfileDir, modulesDir: opts.modulesDir }
  )
  const virtualStoreDir = await resolveVirtualStoreDir(opts.virtualStoreDir, opts.lockfileDir, importerContext.rootModulesDir)
  await prepareStoreDir(opts.storeDir, opts.lockfileDir, opts.frozenStore)

  const installStateDir = path.join(importerContext.rootModulesDir, '.pnpm')
  const hoistedDirs = resolveHoistedDirs({
    enableGlobalVirtualStore: opts.enableGlobalVirtualStore,
    installStateDir,
    virtualStoreDir,
    hoistPattern: opts.hoistPattern,
    extraBinPaths: opts.extraBinPaths,
  })

  const hookedManifest = await opts.readPackageHook?.(manifest, opts.dir) ?? manifest
  const importer = importerContext.projects[0]

  const lockfiles = await readLockfiles({
    autoInstallPeers: opts.autoInstallPeers,
    ci: opts.ci,
    excludeLinksFromLockfile: opts.excludeLinksFromLockfile,
    peersSuffixMaxLength: opts.peersSuffixMaxLength,
    force: opts.force,
    frozenLockfile: false,
    lockfileDir: opts.lockfileDir,
    projects: [{ id: importer.id, manifest: hookedManifest, rootDir: opts.dir as ProjectRootDir }],
    registry: opts.registriesByScope.default,
    useLockfile: opts.useLockfile,
    useGitBranchLockfile: opts.useGitBranchLockfile,
    mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
    installStateDir,
  })

  const ctx = buildPnpmSingleContext({ opts, importerContext, hoistedDirs, hookedManifest, importer, virtualStoreDir, lockfiles })

  packageManifestLogger.debug({ initial: manifest, prefix: opts.dir })
  contextLogger.debug({
    currentLockfileExists: ctx.existsCurrentLockfile,
    storeDir: opts.storeDir,
    virtualStoreDir,
  })
  return ctx
}

function buildPnpmSingleContext (params: {
  opts: GetContextForSingleImporterOptions
  importerContext: Awaited<ReturnType<typeof readProjectsContext>>
  hoistedDirs: { extraBinPaths: string[], hoistedModulesDir: string }
  hookedManifest: ProjectManifest
  importer: Awaited<ReturnType<typeof readProjectsContext>>['projects'][0]
  virtualStoreDir: string
  lockfiles: Awaited<ReturnType<typeof readLockfiles>>
}): PnpmSingleContext {
  const { opts, importerContext, hoistedDirs, hookedManifest, importer, virtualStoreDir, lockfiles } = params
  return {
    extraBinPaths: hoistedDirs.extraBinPaths,
    extraNodePaths: getExtraNodePaths({
      extendNodePath: opts.extendNodePath,
      nodeLinker: opts.nodeLinker,
      hoistPattern: importerContext.currentHoistPattern ?? opts.hoistPattern,
      hoistedModulesDir: hoistedDirs.hoistedModulesDir,
    }),
    hoistedDependencies: importerContext.hoistedDependencies,
    hoistedModulesDir: hoistedDirs.hoistedModulesDir,
    hoistPattern: opts.hoistPattern,
    importerId: importer.id,
    include: opts.include ?? importerContext.include,
    lockfileDir: opts.lockfileDir,
    manifest: hookedManifest,
    modulesDir: importer.modulesDir,
    modulesFile: importerContext.modules,
    pendingBuilds: importerContext.pendingBuilds,
    prefix: opts.dir,
    publicHoistPattern: opts.publicHoistPattern,
    registriesByScope: opts.registriesByScope,
    rootModulesDir: importerContext.rootModulesDir,
    skipped: importerContext.skipped,
    storeDir: opts.storeDir,
    virtualStoreDir,
    ...lockfiles,
  }
}

async function resolveVirtualStoreDir (
  virtualStoreDir: string | undefined,
  lockfileDir: string,
  rootModulesDir: string
): Promise<string> {
  if (virtualStoreDir == null) {
    return path.join(rootModulesDir, '.pnpm')
  }
  return realpathMissing(pathAbsolute(virtualStoreDir, lockfileDir))
}

async function prepareStoreDir (storeDir: string, lockfileDir: string, frozenStore?: boolean): Promise<void> {
  if (!frozenStore) {
    await fs.mkdir(storeDir, { recursive: true })
    await registerProject(storeDir, lockfileDir)
  }
}

function logManifests (projects: Array<ProjectOptions & HookOptions>): void {
  for (const project of projects) {
    packageManifestLogger.debug({
      initial: project.manifest,
      prefix: project.rootDir,
    })
  }
}

async function applyReadPackageHook (
  projects: Array<ProjectOptions & HookOptions>,
  readPackageHook?: ReadPackageHook
): Promise<void> {
  if (!readPackageHook) return
  await Promise.all(projects.map(async (project) => {
    project.originalManifest = project.manifest
    project.manifest = await readPackageHook(clone(project.manifest), project.rootDir)
  }))
}

function resolveHoistedDirs (opts: {
  enableGlobalVirtualStore?: boolean
  installStateDir: string
  virtualStoreDir: string
  hoistPattern?: string[]
  extraBinPaths?: string[]
}): { extraBinPaths: string[], hoistedModulesDir: string } {
  const extraBinPaths = [...opts.extraBinPaths || []]
  const hoistedModulesDir = path.join(
    opts.enableGlobalVirtualStore ? opts.installStateDir : opts.virtualStoreDir,
    'node_modules'
  )
  if (opts.hoistPattern?.length) {
    extraBinPaths.unshift(path.join(hoistedModulesDir, '.bin'))
  }
  return { extraBinPaths, hoistedModulesDir }
}

function getExtraNodePaths (
  { extendNodePath = true, hoistPattern, nodeLinker, hoistedModulesDir }: {
    extendNodePath?: boolean
    hoistPattern?: string[]
    nodeLinker: 'isolated' | 'hoisted' | 'pnp'
    hoistedModulesDir: string
  }
): string[] {
  if (extendNodePath && nodeLinker === 'isolated' && hoistPattern?.length) {
    return [hoistedModulesDir]
  }
  return []
}

export function arrayOfWorkspacePackagesToMap (
  pkgs: Array<Pick<ProjectOptions, 'manifest' | 'rootDir'>>
): WorkspacePackages {
  const workspacePkgs: WorkspacePackages = new Map()
  for (const { manifest, rootDir } of pkgs) {
    if (!manifest.name) continue
    let workspacePkgsByVersion = workspacePkgs.get(manifest.name)
    if (!workspacePkgsByVersion) {
      workspacePkgsByVersion = new Map()
      workspacePkgs.set(manifest.name, workspacePkgsByVersion)
    }
    workspacePkgsByVersion.set(manifest.version ?? '0.0.0', {
      manifest: manifest as DependencyManifest,
      rootDir,
    })
  }
  return workspacePkgs
}
