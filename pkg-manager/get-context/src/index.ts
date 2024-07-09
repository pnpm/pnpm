import { promises as fs } from 'fs'
import path from 'path'
import { contextLogger, packageManifestLogger } from '@pnpm/core-loggers'
import { PnpmError } from '@pnpm/error'
import { type Lockfile } from '@pnpm/lockfile-file'
import { logger } from '@pnpm/logger'
import {
  type IncludedDependencies,
  type Modules,
} from '@pnpm/modules-yaml'
import { readProjectsContext } from '@pnpm/read-projects-context'
import { type WorkspacePackages } from '@pnpm/resolver-base'
import {
  type DepPath,
  DEPENDENCIES_FIELDS,
  type HoistedDependencies,
  type ProjectId,
  type ProjectManifest,
  type ReadPackageHook,
  type Registries,
  type DependencyManifest,
  type ProjectRootDir,
  type ProjectRootDirRealPath,
} from '@pnpm/types'
import rimraf from '@zkochan/rimraf'
import { isCI } from 'ci-info'
import enquirer from 'enquirer'
import pathAbsolute from 'path-absolute'
import clone from 'ramda/src/clone'
import equals from 'ramda/src/equals'
import { checkCompatibility } from './checkCompatibility'
import { UnexpectedStoreError } from './checkCompatibility/UnexpectedStoreError'
import { UnexpectedVirtualStoreDirError } from './checkCompatibility/UnexpectedVirtualStoreDirError'
import { readLockfiles } from './readLockfiles'

export { UnexpectedStoreError, UnexpectedVirtualStoreDirError }

export interface PnpmContext {
  currentLockfile: Lockfile
  currentLockfileIsUpToDate: boolean
  existsCurrentLockfile: boolean
  existsWantedLockfile: boolean
  existsNonEmptyWantedLockfile: boolean
  extraBinPaths: string[]
  extraNodePaths: string[]
  lockfileHadConflicts: boolean
  hoistedDependencies: HoistedDependencies
  include: IncludedDependencies
  modulesFile: Modules | null
  pendingBuilds: string[]
  projects: Record<string, {
    modulesDir: string
    id: ProjectId
  } & HookOptions & Required<ProjectOptions>>
  rootModulesDir: string
  hoistPattern: string[] | undefined
  hoistedModulesDir: string
  publicHoistPattern: string[] | undefined
  lockfileDir: string
  virtualStoreDir: string
  virtualStoreDirMaxLength: number
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
  forceNewModules?: boolean
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
interface ImporterToPurge {
  modulesDir: string
  rootDir: ProjectRootDir
}

export async function getContext (
  opts: GetContextOptions
): Promise<PnpmContext> {
  const modulesDir = opts.modulesDir ?? 'node_modules'
  let importersContext = await readProjectsContext(opts.allProjects, { lockfileDir: opts.lockfileDir, modulesDir })
  const virtualStoreDir = pathAbsolute(opts.virtualStoreDir ?? path.join(modulesDir, '.pnpm'), opts.lockfileDir)

  if (importersContext.modules != null) {
    const { purged } = await validateModules(importersContext.modules, importersContext.projects, {
      currentHoistPattern: importersContext.currentHoistPattern,
      currentPublicHoistPattern: importersContext.currentPublicHoistPattern,
      forceNewModules: opts.forceNewModules === true,
      include: opts.include,
      lockfileDir: opts.lockfileDir,
      modulesDir,
      registries: opts.registries,
      storeDir: opts.storeDir,
      virtualStoreDir,
      virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
      confirmModulesPurge: opts.confirmModulesPurge && !isCI,

      forceHoistPattern: opts.forceHoistPattern,
      hoistPattern: opts.hoistPattern,

      forcePublicHoistPattern: opts.forcePublicHoistPattern,
      publicHoistPattern: opts.publicHoistPattern,
      global: opts.global,
    })
    if (purged) {
      importersContext = await readProjectsContext(opts.allProjects, {
        lockfileDir: opts.lockfileDir,
        modulesDir,
      })
    }
  }

  await fs.mkdir(opts.storeDir, { recursive: true })

  opts.allProjects.forEach((project) => {
    packageManifestLogger.debug({
      initial: project.manifest,
      prefix: project.rootDir,
    })
  })
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
  const hoistPattern = importersContext.currentHoistPattern ?? opts.hoistPattern
  const ctx: PnpmContext = {
    extraBinPaths,
    extraNodePaths: getExtraNodePaths({ extendNodePath: opts.extendNodePath, nodeLinker: opts.nodeLinker, hoistPattern, virtualStoreDir }),
    hoistedDependencies: importersContext.hoistedDependencies,
    hoistedModulesDir,
    hoistPattern,
    include: opts.include ?? importersContext.include,
    lockfileDir: opts.lockfileDir,
    modulesFile: importersContext.modules,
    pendingBuilds: importersContext.pendingBuilds,
    projects: Object.fromEntries(importersContext.projects.map((project) => [project.rootDir, project])),
    publicHoistPattern: importersContext.currentPublicHoistPattern ?? opts.publicHoistPattern,
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

async function validateModules (
  modules: Modules,
  projects: Array<{
    modulesDir: string
    id: string
    rootDir: ProjectRootDir
  }>,
  opts: {
    currentHoistPattern?: string[]
    currentPublicHoistPattern?: string[]
    forceNewModules: boolean
    include?: IncludedDependencies
    lockfileDir: string
    modulesDir: string
    registries: Registries
    storeDir: string
    virtualStoreDir: string
    virtualStoreDirMaxLength: number
    confirmModulesPurge?: boolean

    hoistPattern?: string[] | undefined
    forceHoistPattern?: boolean

    publicHoistPattern?: string[] | undefined
    forcePublicHoistPattern?: boolean
    global?: boolean
  }
): Promise<{ purged: boolean }> {
  const rootProject = projects.find(({ id }) => id === '.')
  if (opts.virtualStoreDirMaxLength !== modules.virtualStoreDirMaxLength) {
    if (opts.forceNewModules && (rootProject != null)) {
      await purgeModulesDirsOfImporter(opts, rootProject)
      return { purged: true }
    }
    throw new PnpmError(
      'VIRTUAL_STORE_DIR_MAX_LENGTH_DIFF',
      'This modules directory was created using a different virtual-store-dir-max-length value.' +
      ' Run "pnpm install" to recreate the modules directory.'
    )
  }
  if (
    opts.forcePublicHoistPattern &&
    // eslint-disable-next-line @typescript-eslint/prefer-nullish-coalescing
    !equals(modules.publicHoistPattern, opts.publicHoistPattern || undefined)
  ) {
    if (opts.forceNewModules && (rootProject != null)) {
      await purgeModulesDirsOfImporter(opts, rootProject)
      return { purged: true }
    }
    throw new PnpmError(
      'PUBLIC_HOIST_PATTERN_DIFF',
      'This modules directory was created using a different public-hoist-pattern value.' +
      ' Run "pnpm install" to recreate the modules directory.'
    )
  }

  const importersToPurge: ImporterToPurge[] = []

  if (opts.forceHoistPattern && (rootProject != null)) {
    try {
      // eslint-disable-next-line @typescript-eslint/prefer-nullish-coalescing
      if (!equals(opts.currentHoistPattern, opts.hoistPattern || undefined)) {
        throw new PnpmError(
          'HOIST_PATTERN_DIFF',
          'This modules directory was created using a different hoist-pattern value.' +
          ' Run "pnpm install" to recreate the modules directory.'
        )
      }
    } catch (err: any) { // eslint-disable-line
      if (!opts.forceNewModules) throw err
      importersToPurge.push(rootProject)
    }
  }
  for (const project of projects) {
    try {
      checkCompatibility(modules, {
        modulesDir: project.modulesDir,
        storeDir: opts.storeDir,
        virtualStoreDir: opts.virtualStoreDir,
      })
      if (opts.lockfileDir !== project.rootDir && (opts.include != null) && modules.included) {
        for (const depsField of DEPENDENCIES_FIELDS) {
          if (opts.include[depsField] !== modules.included[depsField]) {
            throw new PnpmError('INCLUDED_DEPS_CONFLICT',
              `modules directory (at "${opts.lockfileDir}") was installed with ${stringifyIncludedDeps(modules.included)}. ` +
              `Current install wants ${stringifyIncludedDeps(opts.include)}.`
            )
          }
        }
      }
    } catch (err: any) { // eslint-disable-line
      if (!opts.forceNewModules) throw err
      importersToPurge.push(project)
    }
  }
  if (importersToPurge.length > 0 && (rootProject == null)) {
    importersToPurge.push({
      modulesDir: path.join(opts.lockfileDir, opts.modulesDir),
      rootDir: opts.lockfileDir as ProjectRootDir,
    })
  }

  const purged = importersToPurge.length > 0
  if (purged) {
    await purgeModulesDirsOfImporters(opts, importersToPurge)
  }

  return { purged }
}

async function purgeModulesDirsOfImporter (
  opts: {
    confirmModulesPurge?: boolean
    virtualStoreDir: string
  },
  importer: ImporterToPurge
): Promise<void> {
  return purgeModulesDirsOfImporters(opts, [importer])
}

async function purgeModulesDirsOfImporters (
  opts: {
    confirmModulesPurge?: boolean
    virtualStoreDir: string
  },
  importers: ImporterToPurge[]
): Promise<void> {
  if (opts.confirmModulesPurge ?? true) {
    const confirmed = await enquirer.prompt({
      type: 'confirm',
      name: 'question',
      message: importers.length === 1
        ? `The modules directory at "${importers[0].modulesDir}" will be removed and reinstalled from scratch. Proceed?`
        : 'The modules directories will be removed and reinstalled from scratch. Proceed?',
      initial: true,
    })
    if (!confirmed) {
      throw new PnpmError('ABORTED_REMOVE_MODULES_DIR', 'Aborted removal of modules directory')
    }
  }
  await Promise.all(importers.map(async (importer) => {
    logger.info({
      message: `Recreating ${importer.modulesDir}`,
      prefix: importer.rootDir,
    })
    try {
      // We don't remove the actual modules directory, just the contents of it.
      // 1. we will need the directory anyway.
      // 2. in some setups, pnpm won't even have permission to remove the modules directory.
      await removeContentsOfDir(importer.modulesDir, opts.virtualStoreDir)
    } catch (err: any) { // eslint-disable-line
      if (err.code !== 'ENOENT') throw err
    }
  }))
}

async function removeContentsOfDir (dir: string, virtualStoreDir: string): Promise<void> {
  const items = await fs.readdir(dir)
  await Promise.all(items.map(async (item) => {
    // The non-pnpm related hidden files are kept
    if (
      item.startsWith('.') &&
      item !== '.bin' &&
      item !== '.modules.yaml' &&
      !dirsAreEqual(path.join(dir, item), virtualStoreDir)
    ) {
      return
    }
    await rimraf(path.join(dir, item))
  }))
}

function dirsAreEqual (dir1: string, dir2: string): boolean {
  return path.relative(dir1, dir2) === ''
}

function stringifyIncludedDeps (included: IncludedDependencies): string {
  return DEPENDENCIES_FIELDS.filter((depsField) => included[depsField]).join(', ')
}

export interface PnpmSingleContext {
  currentLockfile: Lockfile
  currentLockfileIsUpToDate: boolean
  existsCurrentLockfile: boolean
  existsWantedLockfile: boolean
  existsNonEmptyWantedLockfile: boolean
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
  include: IncludedDependencies
  modulesFile: Modules | null
  pendingBuilds: string[]
  publicHoistPattern: string[] | undefined
  registries: Registries
  rootModulesDir: string
  lockfileDir: string
  virtualStoreDir: string
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
    forceNewModules?: boolean
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
  },
  alreadyPurged: boolean = false
): Promise<PnpmSingleContext> {
  const {
    currentHoistPattern,
    currentPublicHoistPattern,
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

  if ((modules != null) && !alreadyPurged) {
    const { purged } = await validateModules(modules, projects, {
      currentHoistPattern,
      currentPublicHoistPattern,
      forceNewModules: opts.forceNewModules === true,
      include: opts.include,
      lockfileDir: opts.lockfileDir,
      modulesDir: opts.modulesDir ?? 'node_modules',
      registries: opts.registries,
      storeDir: opts.storeDir,
      virtualStoreDir,
      virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
      confirmModulesPurge: opts.confirmModulesPurge && !isCI,

      forceHoistPattern: opts.forceHoistPattern,
      hoistPattern: opts.hoistPattern,

      forcePublicHoistPattern: opts.forcePublicHoistPattern,
      publicHoistPattern: opts.publicHoistPattern,
    })
    if (purged) {
      return getContextForSingleImporter(manifest, opts, true)
    }
  }

  await fs.mkdir(storeDir, { recursive: true })
  const extraBinPaths = [
    ...opts.extraBinPaths || [],
  ]
  const hoistedModulesDir = path.join(virtualStoreDir, 'node_modules')
  if (opts.hoistPattern?.length) {
    extraBinPaths.unshift(path.join(hoistedModulesDir, '.bin'))
  }
  const hoistPattern = currentHoistPattern ?? opts.hoistPattern
  const ctx: PnpmSingleContext = {
    extraBinPaths,
    extraNodePaths: getExtraNodePaths({ extendNodePath: opts.extendNodePath, nodeLinker: opts.nodeLinker, hoistPattern, virtualStoreDir }),
    hoistedDependencies,
    hoistedModulesDir,
    hoistPattern,
    importerId,
    include: opts.include ?? include,
    lockfileDir: opts.lockfileDir,
    manifest: await opts.readPackageHook?.(manifest) ?? manifest,
    modulesDir,
    modulesFile: modules,
    pendingBuilds,
    prefix: opts.dir,
    publicHoistPattern: currentPublicHoistPattern ?? opts.publicHoistPattern,
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
