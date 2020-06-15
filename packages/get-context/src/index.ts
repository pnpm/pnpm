import { packageManifestLogger } from '@pnpm/core-loggers'
import PnpmError from '@pnpm/error'
import { Lockfile } from '@pnpm/lockfile-file'
import logger from '@pnpm/logger'
import {
  IncludedDependencies,
  Modules,
} from '@pnpm/modules-yaml'
import readProjectsContext from '@pnpm/read-projects-context'
import {
  DEPENDENCIES_FIELDS,
  HoistedDependencies,
  ProjectManifest,
  ReadPackageHook,
  Registries,
} from '@pnpm/types'
import rimraf = require('@zkochan/rimraf')
import fs = require('mz/fs')
import path = require('path')
import pathAbsolute = require('path-absolute')
import R = require('ramda')
import checkCompatibility from './checkCompatibility'
import readLockfileFile from './readLockfiles'

export interface PnpmContext<T> {
  currentLockfile: Lockfile,
  currentLockfileIsUpToDate: boolean,
  existsCurrentLockfile: boolean,
  existsWantedLockfile: boolean,
  extraBinPaths: string[],
  hoistedDependencies: HoistedDependencies,
  include: IncludedDependencies,
  modulesFile: Modules | null,
  pendingBuilds: string[],
  projects: Array<{
    modulesDir: string,
    id: string,
  } & T & Required<ProjectOptions>>,
  rootModulesDir: string,
  hoistPattern: string[] | undefined,
  hoistedModulesDir: string,
  publicHoistPattern: string[] | undefined,
  lockfileDir: string,
  virtualStoreDir: string,
  skipped: Set<string>,
  storeDir: string,
  wantedLockfile: Lockfile,
  registries: Registries,
}

export interface ProjectOptions {
  binsDir?: string,
  manifest: ProjectManifest,
  modulesDir?: string,
  rootDir: string,
}

export default async function getContext<T> (
  projects: (ProjectOptions & T)[],
  opts: {
    force: boolean,
    forceNewModules?: boolean,
    forceSharedLockfile: boolean,
    extraBinPaths: string[],
    lockfileDir: string,
    modulesDir?: string,
    hooks?: {
      readPackage?: ReadPackageHook,
    },
    include?: IncludedDependencies,
    registries: Registries,
    storeDir: string,
    useLockfile: boolean,
    virtualStoreDir?: string,

    hoistPattern?: string[] | undefined,
    forceHoistPattern?: boolean,

    publicHoistPattern?: string[] | undefined,
    forcePublicHoistPattern?: boolean,
  }
): Promise<PnpmContext<T>> {
  const modulesDir = opts.modulesDir ?? 'node_modules'
  let importersContext = await readProjectsContext(projects, { lockfileDir: opts.lockfileDir, modulesDir })
  const virtualStoreDir = pathAbsolute(opts.virtualStoreDir ?? path.join(modulesDir, '.pnpm'), opts.lockfileDir)

  if (importersContext.modules) {
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

      forceHoistPattern: opts.forceHoistPattern,
      hoistPattern: opts.hoistPattern,

      forcePublicHoistPattern: opts.forcePublicHoistPattern,
      publicHoistPattern: opts.publicHoistPattern,
    })
    if (purged) {
      importersContext = await readProjectsContext(projects, {
        lockfileDir: opts.lockfileDir,
        modulesDir,
      })
    }
  }

  await fs.mkdir(opts.storeDir, { recursive: true })

  projects.forEach((project) => {
    packageManifestLogger.debug({
      initial: project.manifest,
      prefix: project.rootDir,
    })
  })
  if (opts.hooks?.readPackage) {
    projects = projects.map((project) => ({
      ...project,
      manifest: opts.hooks!.readPackage!(project.manifest),
    }))
  }

  const extraBinPaths = [
    ...opts.extraBinPaths || [],
  ]
  const hoistedModulesDir = path.join(virtualStoreDir, 'node_modules')
  if (opts.hoistPattern?.length) {
    extraBinPaths.unshift(path.join(hoistedModulesDir, '.bin'))
  }
  const ctx: PnpmContext<T> = {
    extraBinPaths,
    hoistedDependencies: importersContext.hoistedDependencies,
    hoistedModulesDir,
    hoistPattern: importersContext.currentHoistPattern ?? opts.hoistPattern,
    include: opts.include || importersContext.include,
    lockfileDir: opts.lockfileDir,
    modulesFile: importersContext.modules,
    pendingBuilds: importersContext.pendingBuilds,
    projects: importersContext.projects,
    publicHoistPattern: importersContext.currentPublicHoistPattern ?? opts.publicHoistPattern,
    registries: {
      ...opts.registries,
      ...importersContext.registries,
    },
    rootModulesDir: importersContext.rootModulesDir,
    skipped: importersContext.skipped,
    storeDir: opts.storeDir,
    virtualStoreDir,
    ...await readLockfileFile({
      force: opts.force,
      forceSharedLockfile: opts.forceSharedLockfile,
      lockfileDir: opts.lockfileDir,
      projects: importersContext.projects,
      registry: opts.registries.default,
      useLockfile: opts.useLockfile,
      virtualStoreDir,
    }),
  }

  return ctx
}

async function validateModules (
  modules: Modules,
  projects: Array<{
    modulesDir: string,
    id: string,
    rootDir: string,
  }>,
  opts: {
    currentHoistPattern?: string[],
    currentPublicHoistPattern?: string[],
    forceNewModules: boolean,
    include?: IncludedDependencies,
    lockfileDir: string,
    modulesDir: string,
    registries: Registries,
    storeDir: string,
    virtualStoreDir: string,

    hoistPattern?: string[] | undefined,
    forceHoistPattern?: boolean,

    publicHoistPattern?: string[] | undefined,
    forcePublicHoistPattern?: boolean,
  }
): Promise<{ purged: boolean }> {
  const rootProject = projects.find(({ id }) => id === '.')
  if (opts.forcePublicHoistPattern && !R.equals(modules.publicHoistPattern, opts.publicHoistPattern)) {
    if (opts.forceNewModules && rootProject) {
      await purgeModulesDirsOfImporter(rootProject)
      return { purged: true }
    }
    throw new PnpmError(
      'PUBLIC_HOIST_PATTERN_DIFF',
      'This modules directory was created using a different public-hoist-pattern value.'
      + ' Run "pnpm install" to recreate the modules directory.'
    )
  }
  let purged = false
  if (opts.forceHoistPattern && rootProject) {
    try {
      if (!R.equals(opts.currentHoistPattern, (opts.hoistPattern || undefined))) {
        throw new PnpmError(
          'HOIST_PATTERN_DIFF',
          'This modules directory was created using a different hoist-pattern value.'
          + ' Run "pnpm install" to recreate the modules directory.'
        )
      }
    } catch (err) {
      if (!opts.forceNewModules) throw err
      await purgeModulesDirsOfImporter(rootProject)
      purged = true
    }
  }
  await Promise.all(projects.map(async (project) => {
    try {
      checkCompatibility(modules, {
        modulesDir: project.modulesDir,
        storeDir: opts.storeDir,
        virtualStoreDir: opts.virtualStoreDir,
      })
      if (opts.lockfileDir !== project.rootDir && opts.include && modules.included) {
        for (const depsField of DEPENDENCIES_FIELDS) {
          if (opts.include[depsField] !== modules.included[depsField]) {
            throw new PnpmError('INCLUDED_DEPS_CONFLICT',
              `modules directory (at "${opts.lockfileDir}") was installed with ${stringifyIncludedDeps(modules.included)}. ` +
              `Current install wants ${stringifyIncludedDeps(opts.include)}.`
            )
          }
        }
      }
    } catch (err) {
      if (!opts.forceNewModules) throw err
      await purgeModulesDirsOfImporter(project)
      purged = true
    }
  }))
  if (modules.registries && !R.equals(opts.registries, modules.registries)) {
    if (opts.forceNewModules) {
      await Promise.all(projects.map(purgeModulesDirsOfImporter))
      return { purged: true }
    }
    throw new PnpmError('REGISTRIES_MISMATCH', `This modules directory was created using the following registries configuration: ${JSON.stringify(modules.registries)}. The current configuration is ${JSON.stringify(opts.registries)}. To recreate recreate the modules directory using the new settings, run "pnpm install".`)
  }
  if (purged && !rootProject) {
    await purgeModulesDirsOfImporter({
      modulesDir: path.join(opts.lockfileDir, opts.modulesDir),
      rootDir: opts.lockfileDir,
    })
  }
  return { purged }
}

async function purgeModulesDirsOfImporter (
  importer: {
    modulesDir: string,
    rootDir: string,
  }
) {
  logger.info({
    message: `Recreating ${importer.modulesDir}`,
    prefix: importer.rootDir,
  })
  try {
    await rimraf(importer.modulesDir)
  } catch (err) {
    if (err.code !== 'ENOENT') throw err
  }
}

function stringifyIncludedDeps (included: IncludedDependencies) {
  return DEPENDENCIES_FIELDS.filter((depsField) => included[depsField]).join(', ')
}

export interface PnpmSingleContext {
  currentLockfile: Lockfile,
  currentLockfileIsUpToDate: boolean,
  existsCurrentLockfile: boolean,
  existsWantedLockfile: boolean,
  extraBinPaths: string[],
  hoistedDependencies: HoistedDependencies,
  hoistedModulesDir: string,
  hoistPattern: string[] | undefined,
  manifest: ProjectManifest,
  modulesDir: string,
  importerId: string,
  prefix: string,
  include: IncludedDependencies,
  modulesFile: Modules | null,
  pendingBuilds: string[],
  publicHoistPattern: string[] | undefined,
  registries: Registries,
  rootModulesDir: string,
  lockfileDir: string,
  virtualStoreDir: string,
  skipped: Set<string>,
  storeDir: string,
  wantedLockfile: Lockfile,
}

export async function getContextForSingleImporter (
  manifest: ProjectManifest,
  opts: {
    force: boolean,
    forceNewModules?: boolean,
    forceSharedLockfile: boolean,
    extraBinPaths: string[],
    lockfileDir: string,
    modulesDir?: string,
    hooks?: {
      readPackage?: ReadPackageHook,
    },
    include?: IncludedDependencies,
    dir: string,
    registries: Registries,
    storeDir: string,
    useLockfile: boolean,
    virtualStoreDir?: string,

    hoistPattern?: string[] | undefined,
    forceHoistPattern?: boolean,

    publicHoistPattern?: string[] | undefined,
    forcePublicHoistPattern?: boolean,
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
        rootDir: opts.dir,
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

  if (modules && !alreadyPurged) {
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
  const ctx: PnpmSingleContext = {
    extraBinPaths,
    hoistedDependencies,
    hoistedModulesDir,
    hoistPattern: currentHoistPattern ?? opts.hoistPattern,
    importerId,
    include: opts.include || include,
    lockfileDir: opts.lockfileDir,
    manifest: opts.hooks?.readPackage?.(manifest) ?? manifest,
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
    ...await readLockfileFile({
      force: opts.force,
      forceSharedLockfile: opts.forceSharedLockfile,
      lockfileDir: opts.lockfileDir,
      projects: [{ id: importerId, rootDir: opts.dir }],
      registry: opts.registries.default,
      useLockfile: opts.useLockfile,
      virtualStoreDir,
    }),
  }
  packageManifestLogger.debug({
    initial: manifest,
    prefix: opts.dir,
  })

  return ctx
}
