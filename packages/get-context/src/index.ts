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
  const importersContext = await readProjectsContext(projects, opts.lockfileDir)
  const virtualStoreDir = pathAbsolute(opts.virtualStoreDir ?? 'node_modules/.pnpm', opts.lockfileDir)

  if (importersContext.modules) {
    await validateNodeModules(importersContext.modules, importersContext.projects, {
      currentHoistPattern: importersContext.currentHoistPattern,
      forceNewNodeModules: opts.forceNewNodeModules === true,
      include: opts.include,
      lockfileDir: opts.lockfileDir,
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
    hoistPattern: typeof importersContext.hoist === 'boolean' ?
      importersContext.currentHoistPattern : opts.hoistPattern,
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
) {
  const rootProject = projects.find(({ id }) => id === '.')
  if (opts.forceShamefullyHoist && modules.shamefullyHoist !== opts.shamefullyHoist) {
    if (opts.forceNewNodeModules && rootProject) {
      await purgeModulesDirsOfImporter(rootProject)
      return
    }
    if (modules.shamefullyHoist) {
      throw new PnpmError(
        'SHAMEFULLY_HOIST_WANTED',
        'This "node_modules" folder was created using the --shamefully-hoist option.'
        + ' You must add that option, or else run "pnpm install" to recreate the "node_modules" folder.',
      )
    }
    throw new PnpmError(
      'SHAMEFULLY_HOIST_NOT_WANTED',
      'This "node_modules" folder was created without the --shamefully-hoist option.'
      + ' You must remove that option, or else "pnpm install" to recreate the "node_modules" folder.',
    )
  }
  if (opts.forceIndependentLeaves && Boolean(modules.independentLeaves) !== opts.independentLeaves) {
    if (opts.forceNewNodeModules) {
      // TODO: remove the node_modules in the lockfile directory
      await Promise.all(projects.map(purgeModulesDirsOfImporter))
      return
    }
    if (modules.independentLeaves) {
      throw new PnpmError(
        'INDEPENDENT_LEAVES_WANTED',
        'This "node_modules" folder was created using the --independent-leaves option.'
        + ' You must add that option, or else run "pnpm install" to recreate the "node_modules" folder.',
      )
    }
    throw new PnpmError(
      'INDEPENDENT_LEAVES_NOT_WANTED',
      'This "node_modules" folder was created without the --independent-leaves option.'
      + ' You must remove that option, or else "pnpm install" to recreate the "node_modules" folder.',
    )
  }
  if (opts.forceHoistPattern && rootProject) {
    try {
      if (!R.equals(opts.currentHoistPattern, (opts.hoistPattern || undefined))) {
        if (opts.currentHoistPattern) {
          throw new PnpmError(
            'HOISTING_WANTED',
            'This "node_modules" folder was created using the --hoist-pattern option.'
            + ' You must add this option, or else add the --force option to recreate the "node_modules" folder.',
          )
        }
        throw new PnpmError(
          'HOISTING_NOT_WANTED',
          'This "node_modules" folder was created without the --hoist-pattern option.'
          + ' You must remove that option, or else run "pnpm install" to recreate the "node_modules" folder.',
        )
      }
    } catch (err) {
      if (!opts.forceNewNodeModules) throw err
      await purgeModulesDirsOfImporter(rootProject)
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
              `node_modules (at "${opts.lockfileDir}") was installed with ${stringifyIncludedDeps(modules.included)}. ` +
              `Current install wants ${stringifyIncludedDeps(opts.include)}.`,
            )
          }
        }
      }
    } catch (err) {
      if (!opts.forceNewNodeModules) throw err
      await purgeModulesDirsOfImporter(project)
    }
  }))
  if (modules.registries && !R.equals(opts.registries, modules.registries)) {
    if (opts.forceNewNodeModules) {
      await Promise.all(projects.map(purgeModulesDirsOfImporter))
      return
    }
    throw new PnpmError('REGISTRIES_MISMATCH', `This "node_modules" directory was created using the following registries configuration: ${JSON.stringify(modules.registries)}. The current configuration is ${JSON.stringify(opts.registries)}. To recreate "node_modules" using the new settings, run "pnpm install".`)
  }
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
): Promise<PnpmSingleContext> {
  const {
    currentHoistPattern,
    hoist,
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
    opts.lockfileDir,
  )

  const storeDir = opts.storeDir

  const importer = projects[0]
  const modulesDir = importer.modulesDir
  const importerId = importer.id
  const virtualStoreDir = pathAbsolute(opts.virtualStoreDir ?? 'node_modules/.pnpm', opts.lockfileDir)

  if (modules) {
    await validateNodeModules(modules, projects, {
      currentHoistPattern,
      forceNewNodeModules: opts.forceNewNodeModules === true,
      include: opts.include,
      lockfileDir: opts.lockfileDir,
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
    hoistPattern: typeof hoist === 'boolean' ? currentHoistPattern : opts.hoistPattern,
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
