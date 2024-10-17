import { promises as fs } from 'fs'
import path from 'path'
import { contextLogger, packageManifestLogger } from '@pnpm/core-loggers'
import { type Lockfile } from '@pnpm/lockfile.fs'
import {
  type IncludedDependencies,
  type Modules,
} from '@pnpm/modules-yaml'
import { readProjectsContext } from '@pnpm/read-projects-context'
import { type WorkspacePackages } from '@pnpm/resolver-base'
import {
  type DepPath,
  type HoistedDependencies,
  type ProjectId,
  type ProjectManifest,
  type ReadPackageHook,
  type Registries,
  type DependencyManifest,
  type ProjectRootDir,
  type ProjectRootDirRealPath,
} from '@pnpm/types'
import pathAbsolute from 'path-absolute'
import clone from 'ramda/src/clone'
import { readLockfiles } from './readLockfiles'

/**
 * Note that some fields are affected by modules directory state. Such fields should be used for
 * mutating the modules directory only or in a manner that does not influence dependency resolution.
 */
export interface PnpmContext {
  currentLockfile: Lockfile
  currentLockfileIsUpToDate: boolean
  existsCurrentLockfile: boolean
  existsWantedLockfile: boolean
  existsNonEmptyWantedLockfile: boolean
  extraBinPaths: string[]
  /** Affected by existing modules directory, if it exists. */
  extraNodePaths: string[]
  lockfileHadConflicts: boolean
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
  wantedLockfile: Lockfile
  wantedLockfileIsModified: boolean
  workspacePackages: WorkspacePackages
  registries: Registries
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
  excludeLinksFromLockfile: boolean
  peersSuffixMaxLength: number
  allProjects: Array<ProjectOptions & HookOptions>
  confirmModulesPurge?: boolean
  force: boolean
  frozenLockfile?: boolean
  extraBinPaths: string[]
  extendNodePath?: boolean
  lockfileDir: string
  modulesDir?: string
  nodeLinker: 'isolated' | 'hoisted' | 'pnp'
  readPackageHook?: ReadPackageHook
  include?: IncludedDependencies
  registries: Registries
  storeDir: string
  useLockfile: boolean
  useGitBranchLockfile?: boolean
  mergeGitBranchLockfiles?: boolean
  virtualStoreDir?: string
  virtualStoreDirMaxLength: number
  workspacePackages?: WorkspacePackages

  hoistPattern?: string[] | undefined
  forceHoistPattern?: boolean

  publicHoistPattern?: string[] | undefined
  forcePublicHoistPattern?: boolean
  global?: boolean
}

export async function getContext (
  opts: GetContextOptions
): Promise<PnpmContext> {
  const modulesDir = opts.modulesDir ?? 'node_modules'
  const importersContext = await readProjectsContext(opts.allProjects, { lockfileDir: opts.lockfileDir, modulesDir })
  const virtualStoreDir = pathAbsolute(opts.virtualStoreDir ?? path.join(modulesDir, '.pnpm'), opts.lockfileDir)

  await fs.mkdir(opts.storeDir, { recursive: true })

  for (const project of opts.allProjects) {
    packageManifestLogger.debug({
      initial: project.manifest,
      prefix: project.rootDir,
    })
  }
  if (opts.readPackageHook != null) {
    await Promise.all(importersContext.projects.map(async (project) => {
      project.originalManifest = project.manifest
      project.manifest = await opts.readPackageHook!(clone(project.manifest), project.rootDir)
    }))
  }

  const extraBinPaths = [
    ...opts.extraBinPaths || [],
  ]
  const hoistedModulesDir = path.join(virtualStoreDir, 'node_modules')
  if (opts.hoistPattern?.length) {
    extraBinPaths.unshift(path.join(hoistedModulesDir, '.bin'))
  }
  const ctx: PnpmContext = {
    extraBinPaths,
    extraNodePaths: getExtraNodePaths({ extendNodePath: opts.extendNodePath, nodeLinker: opts.nodeLinker, hoistPattern: importersContext.currentHoistPattern ?? opts.hoistPattern, virtualStoreDir }),
    hoistedDependencies: importersContext.hoistedDependencies,
    hoistedModulesDir,
    hoistPattern: opts.hoistPattern,
    currentHoistPattern: importersContext.currentHoistPattern,
    include: opts.include ?? importersContext.include,
    lockfileDir: opts.lockfileDir,
    modulesFile: importersContext.modules,
    pendingBuilds: importersContext.pendingBuilds,
    projects: Object.fromEntries(importersContext.projects.map((project) => [project.rootDir, project])),
    publicHoistPattern: opts.publicHoistPattern,
    currentPublicHoistPattern: importersContext.currentPublicHoistPattern,
    registries: opts.registries,
    rootModulesDir: importersContext.rootModulesDir,
    skipped: importersContext.skipped,
    storeDir: opts.storeDir,
    virtualStoreDir,
    virtualStoreDirMaxLength: importersContext.virtualStoreDirMaxLength ?? opts.virtualStoreDirMaxLength,
    workspacePackages: opts.workspacePackages ?? arrayOfWorkspacePackagesToMap(opts.allProjects),
    ...await readLockfiles({
      autoInstallPeers: opts.autoInstallPeers,
      excludeLinksFromLockfile: opts.excludeLinksFromLockfile,
      peersSuffixMaxLength: opts.peersSuffixMaxLength,
      force: opts.force,
      frozenLockfile: opts.frozenLockfile === true,
      lockfileDir: opts.lockfileDir,
      projects: importersContext.projects,
      registry: opts.registries.default,
      useLockfile: opts.useLockfile,
      useGitBranchLockfile: opts.useGitBranchLockfile,
      mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
      virtualStoreDir,
    }),
  }
  contextLogger.debug({
    currentLockfileExists: ctx.existsCurrentLockfile,
    storeDir: opts.storeDir,
    virtualStoreDir,
  })
  return ctx
}

export interface PnpmSingleContext {
  currentLockfile: Lockfile
  currentLockfileIsUpToDate: boolean
  existsCurrentLockfile: boolean
  existsWantedLockfile: boolean
  existsNonEmptyWantedLockfile: boolean
  /** Affected by existing modules directory, if it exists. */
  extraBinPaths: string[]
  extraNodePaths: string[]
  lockfileHadConflicts: boolean
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
  registries: Registries
  rootModulesDir: string
  lockfileDir: string
  virtualStoreDir: string
  /** As applied to existing modules directory, if it exists. */
  skipped: Set<string>
  storeDir: string
  wantedLockfile: Lockfile
  wantedLockfileIsModified: boolean
}

export async function getContextForSingleImporter (
  manifest: ProjectManifest,
  opts: {
    autoInstallPeers: boolean
    excludeLinksFromLockfile: boolean
    peersSuffixMaxLength: number
    force: boolean
    confirmModulesPurge?: boolean
    extraBinPaths: string[]
    extendNodePath?: boolean
    lockfileDir: string
    nodeLinker: 'isolated' | 'hoisted' | 'pnp'
    modulesDir?: string
    readPackageHook?: ReadPackageHook
    include?: IncludedDependencies
    dir: string
    registries: Registries
    storeDir: string
    useLockfile: boolean
    useGitBranchLockfile?: boolean
    mergeGitBranchLockfiles?: boolean
    virtualStoreDir?: string
    virtualStoreDirMaxLength: number

    hoistPattern?: string[] | undefined
    forceHoistPattern?: boolean

    publicHoistPattern?: string[] | undefined
    forcePublicHoistPattern?: boolean
  }
): Promise<PnpmSingleContext> {
  const {
    currentHoistPattern,
    hoistedDependencies,
    projects,
    include,
    modules,
    pendingBuilds,
    registries,
    skipped,
    rootModulesDir,
  } = await readProjectsContext(
    [
      {
        rootDir: opts.dir as ProjectRootDir,
      },
    ],
    {
      lockfileDir: opts.lockfileDir,
      modulesDir: opts.modulesDir,
    }
  )

  const storeDir = opts.storeDir

  const importer = projects[0]
  const modulesDir = importer.modulesDir
  const importerId = importer.id
  const virtualStoreDir = pathAbsolute(opts.virtualStoreDir ?? 'node_modules/.pnpm', opts.lockfileDir)

  await fs.mkdir(storeDir, { recursive: true })
  const extraBinPaths = [
    ...opts.extraBinPaths || [],
  ]
  const hoistedModulesDir = path.join(virtualStoreDir, 'node_modules')
  if (opts.hoistPattern?.length) {
    extraBinPaths.unshift(path.join(hoistedModulesDir, '.bin'))
  }
  const ctx: PnpmSingleContext = {
    extraBinPaths,
    extraNodePaths: getExtraNodePaths({ extendNodePath: opts.extendNodePath, nodeLinker: opts.nodeLinker, hoistPattern: currentHoistPattern ?? opts.hoistPattern, virtualStoreDir }),
    hoistedDependencies,
    hoistedModulesDir,
    hoistPattern: opts.hoistPattern,
    importerId,
    include: opts.include ?? include,
    lockfileDir: opts.lockfileDir,
    manifest: await opts.readPackageHook?.(manifest) ?? manifest,
    modulesDir,
    modulesFile: modules,
    pendingBuilds,
    prefix: opts.dir,
    publicHoistPattern: opts.publicHoistPattern,
    registries: {
      ...opts.registries,
      ...registries,
    },
    rootModulesDir,
    skipped,
    storeDir,
    virtualStoreDir,
    ...await readLockfiles({
      autoInstallPeers: opts.autoInstallPeers,
      excludeLinksFromLockfile: opts.excludeLinksFromLockfile,
      peersSuffixMaxLength: opts.peersSuffixMaxLength,
      force: opts.force,
      frozenLockfile: false,
      lockfileDir: opts.lockfileDir,
      projects: [{ id: importerId, rootDir: opts.dir as ProjectRootDir }],
      registry: opts.registries.default,
      useLockfile: opts.useLockfile,
      useGitBranchLockfile: opts.useGitBranchLockfile,
      mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
      virtualStoreDir,
    }),
  }
  packageManifestLogger.debug({
    initial: manifest,
    prefix: opts.dir,
  })
  contextLogger.debug({
    currentLockfileExists: ctx.existsCurrentLockfile,
    storeDir: opts.storeDir,
    virtualStoreDir,
  })

  return ctx
}

function getExtraNodePaths (
  { extendNodePath = true, hoistPattern, nodeLinker, virtualStoreDir }: {
    extendNodePath?: boolean
    hoistPattern?: string[]
    nodeLinker: 'isolated' | 'hoisted' | 'pnp'
    virtualStoreDir: string
  }
): string[] {
  if (extendNodePath && nodeLinker === 'isolated' && hoistPattern?.length) {
    return [path.join(virtualStoreDir, 'node_modules')]
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
