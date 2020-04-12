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
  ProjectManifest,
  ReadPackageHook,
  Registries,
} from '@pnpm/types'
import rimraf = require('@zkochan/rimraf')
import makeDir = require('make-dir')
import path = require('path')
import pathAbsolute = require('path-absolute')
import R = require('ramda')
import checkCompatibility from './checkCompatibility'
import readLockfileFile from './readLockfiles'

export interface PnpmContext<T> {
  currentLockfile: Lockfile,
  existsCurrentLockfile: boolean,
  existsWantedLockfile: boolean,
  extraBinPaths: string[],
  hoistedAliases: {[depPath: string]: string[]}
  include: IncludedDependencies,
  independentLeaves: boolean,
  modulesFile: Modules | null,
  pendingBuilds: string[],
  projects: Array<{
    modulesDir: string,
    id: string,
  } & T & Required<ProjectOptions>>,
  rootModulesDir: string,
  hoistPattern: string[] | undefined,
  hoistedModulesDir: string,
  lockfileDir: string,
  virtualStoreDir: string,
  shamefullyHoist: boolean,
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
    forceNewNodeModules?: boolean,
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

    independentLeaves?: boolean,
    forceIndependentLeaves?: boolean,

    hoistPattern?: string[] | undefined,
    forceHoistPattern?: boolean,

    shamefullyHoist?: boolean,
    forceShamefullyHoist?: boolean,
  },
): Promise<PnpmContext<T>> {
  const modulesDir = opts.modulesDir ?? 'node_modules'
  let importersContext = await readProjectsContext(projects, { lockfileDir: opts.lockfileDir, modulesDir })
  const virtualStoreDir = pathAbsolute(opts.virtualStoreDir ?? path.join(modulesDir, '.pnpm'), opts.lockfileDir)

  if (importersContext.modules) {
    const { purged } = await validateNodeModules(importersContext.modules, importersContext.projects, {
      currentHoistPattern: importersContext.currentHoistPattern,
      forceNewNodeModules: opts.forceNewNodeModules === true,
      include: opts.include,
      lockfileDir: opts.lockfileDir,
      modulesDir,
      registries: opts.registries,
      storeDir: opts.storeDir,
      virtualStoreDir,

      forceIndependentLeaves: opts.forceIndependentLeaves,
      independentLeaves: opts.independentLeaves,

      forceHoistPattern: opts.forceHoistPattern,
      hoistPattern: opts.hoistPattern,

      forceShamefullyHoist: opts.forceShamefullyHoist,
      shamefullyHoist: opts.shamefullyHoist,
    })
    if (purged) {
      importersContext = await readProjectsContext(projects, {
        lockfileDir: opts.lockfileDir,
        modulesDir,
      })
    }
  }

  await makeDir(opts.storeDir)

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
  const shamefullyHoist = Boolean(typeof importersContext.shamefullyHoist === 'undefined' ? opts.shamefullyHoist : importersContext.shamefullyHoist)
  if (opts.hoistPattern && !shamefullyHoist) {
    extraBinPaths.unshift(path.join(virtualStoreDir, 'node_modules/.bin'))
  }
  const hoistedModulesDir = shamefullyHoist
    ? importersContext.rootModulesDir : path.join(virtualStoreDir, 'node_modules')
  const ctx: PnpmContext<T> = {
    extraBinPaths,
    hoistedAliases: importersContext.hoistedAliases,
    hoistedModulesDir,
    hoistPattern: opts.hoistPattern,
    include: opts.include || importersContext.include,
    independentLeaves: Boolean(typeof importersContext.independentLeaves === 'undefined' ? opts.independentLeaves : importersContext.independentLeaves),
    lockfileDir: opts.lockfileDir,
    modulesFile: importersContext.modules,
    pendingBuilds: importersContext.pendingBuilds,
    projects: importersContext.projects,
    registries: {
      ...opts.registries,
      ...importersContext.registries,
    },
    rootModulesDir: importersContext.rootModulesDir,
    shamefullyHoist,
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

async function validateNodeModules (
  modules: Modules,
  projects: Array<{
    modulesDir: string,
    id: string,
    rootDir: string,
  }>,
  opts: {
    currentHoistPattern?: string[],
    forceNewNodeModules: boolean,
    include?: IncludedDependencies,
    lockfileDir: string,
    modulesDir: string,
    registries: Registries,
    storeDir: string,
    virtualStoreDir: string,

    independentLeaves?: boolean,
    forceIndependentLeaves?: boolean,

    hoistPattern?: string[] | undefined,
    forceHoistPattern?: boolean,

    shamefullyHoist?: boolean | undefined,
    forceShamefullyHoist?: boolean,
  },
): Promise<{ purged: boolean }> {
  const rootProject = projects.find(({ id }) => id === '.')
  if (opts.forceShamefullyHoist && modules.shamefullyHoist !== opts.shamefullyHoist) {
    if (opts.forceNewNodeModules && rootProject) {
      await purgeModulesDirsOfImporter(rootProject)
      return { purged: true }
    }
    if (modules.shamefullyHoist) {
      throw new PnpmError(
        'SHAMEFULLY_HOIST_WANTED',
        'This modules directory was created using the --shamefully-hoist option.'
        + ' You must add that option, or else run "pnpm install" to recreate the modules directory.',
      )
    }
    throw new PnpmError(
      'SHAMEFULLY_HOIST_NOT_WANTED',
      'This modules directory was created without the --shamefully-hoist option.'
      + ' You must remove that option, or else "pnpm install" to recreate the modules directory.',
    )
  }
  if (opts.forceIndependentLeaves && Boolean(modules.independentLeaves) !== opts.independentLeaves) {
    if (opts.forceNewNodeModules) {
      await Promise.all(projects.map(purgeModulesDirsOfImporter))
      if (!rootProject) {
        await purgeModulesDirsOfImporter({
          modulesDir: path.join(opts.lockfileDir, opts.modulesDir),
          rootDir: opts.lockfileDir,
        })
      }
      return { purged: true }
    }
    if (modules.independentLeaves) {
      throw new PnpmError(
        'INDEPENDENT_LEAVES_WANTED',
        'This modules directory was created using the --independent-leaves option.'
        + ' You must add that option, or else run "pnpm install" to recreate the modules directory.',
      )
    }
    throw new PnpmError(
      'INDEPENDENT_LEAVES_NOT_WANTED',
      'This modules directory was created without the --independent-leaves option.'
      + ' You must remove that option, or else "pnpm install" to recreate the modules directory.',
    )
  }
  let purged = false
  if (opts.forceHoistPattern && rootProject) {
    try {
      if (!R.equals(opts.currentHoistPattern, (opts.hoistPattern || undefined))) {
        if (opts.currentHoistPattern) {
          throw new PnpmError(
            'HOISTING_WANTED',
            'This modules directory was created using the --hoist-pattern option.'
            + ' You must add this option, or else add the --force option to recreate the modules directory.',
          )
        }
        throw new PnpmError(
          'HOISTING_NOT_WANTED',
          'This modules directory was created without the --hoist-pattern option.'
          + ' You must remove that option, or else run "pnpm install" to recreate the modules directory.',
        )
      }
    } catch (err) {
      if (!opts.forceNewNodeModules) throw err
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
              `Current install wants ${stringifyIncludedDeps(opts.include)}.`,
            )
          }
        }
      }
    } catch (err) {
      if (!opts.forceNewNodeModules) throw err
      await purgeModulesDirsOfImporter(project)
      purged = true
    }
  }))
  if (modules.registries && !R.equals(opts.registries, modules.registries)) {
    if (opts.forceNewNodeModules) {
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
  },
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
  existsCurrentLockfile: boolean,
  existsWantedLockfile: boolean,
  extraBinPaths: string[],
  hoistedAliases: {[depPath: string]: string[]},
  hoistedModulesDir: string,
  hoistPattern: string[] | undefined,
  manifest: ProjectManifest,
  modulesDir: string,
  importerId: string,
  prefix: string,
  include: IncludedDependencies,
  independentLeaves: boolean,
  modulesFile: Modules | null,
  pendingBuilds: string[],
  registries: Registries,
  rootModulesDir: string,
  lockfileDir: string,
  virtualStoreDir: string,
  shamefullyHoist: boolean,
  skipped: Set<string>,
  storeDir: string,
  wantedLockfile: Lockfile,
}

export async function getContextForSingleImporter (
  manifest: ProjectManifest,
  opts: {
    force: boolean,
    forceNewNodeModules?: boolean,
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

    shamefullyHoist?: boolean,
    forceShamefullyHoist?: boolean,

    independentLeaves?: boolean,
    forceIndependentLeaves?: boolean,
  },
  alreadyPurged: boolean = false,
): Promise<PnpmSingleContext> {
  const {
    currentHoistPattern,
    hoistedAliases,
    projects,
    include,
    independentLeaves,
    modules,
    pendingBuilds,
    registries,
    shamefullyHoist,
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
    },
  )

  const storeDir = opts.storeDir

  const importer = projects[0]
  const modulesDir = importer.modulesDir
  const importerId = importer.id
  const virtualStoreDir = pathAbsolute(opts.virtualStoreDir ?? 'node_modules/.pnpm', opts.lockfileDir)

  if (modules && !alreadyPurged) {
    const { purged } = await validateNodeModules(modules, projects, {
      currentHoistPattern,
      forceNewNodeModules: opts.forceNewNodeModules === true,
      include: opts.include,
      lockfileDir: opts.lockfileDir,
      modulesDir: opts.modulesDir ?? 'node_modules',
      registries: opts.registries,
      storeDir: opts.storeDir,
      virtualStoreDir,

      forceHoistPattern: opts.forceHoistPattern,
      hoistPattern: opts.hoistPattern,

      forceIndependentLeaves: opts.forceIndependentLeaves,
      independentLeaves: opts.independentLeaves,

      forceShamefullyHoist: opts.forceShamefullyHoist,
      shamefullyHoist: opts.shamefullyHoist,
    })
    if (purged) {
      return getContextForSingleImporter(manifest, opts, true)
    }
  }

  await makeDir(storeDir)
  const extraBinPaths = [
    ...opts.extraBinPaths || [],
  ]
  const sHoist = Boolean(typeof shamefullyHoist === 'undefined' ? opts.shamefullyHoist : shamefullyHoist)
  if (opts.hoistPattern && !sHoist) {
    extraBinPaths.unshift(path.join(virtualStoreDir, 'node_modules/.bin'))
  }
  const hoistedModulesDir = sHoist
    ? rootModulesDir : path.join(virtualStoreDir, 'node_modules')
  const ctx: PnpmSingleContext = {
    extraBinPaths,
    hoistedAliases,
    hoistedModulesDir,
    hoistPattern: opts.hoistPattern,
    importerId,
    include: opts.include || include,
    independentLeaves: Boolean(typeof independentLeaves === 'undefined' ? opts.independentLeaves : independentLeaves),
    lockfileDir: opts.lockfileDir,
    manifest: opts.hooks?.readPackage?.(manifest) ?? manifest,
    modulesDir,
    modulesFile: modules,
    pendingBuilds,
    prefix: opts.dir,
    registries: {
      ...opts.registries,
      ...registries,
    },
    rootModulesDir,
    shamefullyHoist: sHoist,
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
