import { packageJsonLogger } from '@pnpm/core-loggers'
import logger from '@pnpm/logger'
import {
  IncludedDependencies,
  Modules,
  read as readModulesYaml,
} from '@pnpm/modules-yaml'
import {
  DEPENDENCIES_FIELDS,
  PackageJson,
  ReadPackageHook,
} from '@pnpm/types'
import {
  realNodeModulesDir,
  safeReadPackageFromDir as safeReadPkgFromDir,
} from '@pnpm/utils'
import mkdirp = require('mkdirp-promise')
import {
  getImporterPath,
  Shrinkwrap,
} from 'pnpm-shrinkwrap'
import removeAllExceptOuterLinks = require('remove-all-except-outer-links')
import { PnpmError } from '../errorTypes'
import checkCompatibility from './checkCompatibility'
import readShrinkwrapFile from './readShrinkwrapFiles'

export interface PnpmContext {
  currentShrinkwrap: Shrinkwrap,
  existsCurrentShrinkwrap: boolean,
  existsWantedShrinkwrap: boolean,
  importers: Array<{
    bin: string,
    hoistedAliases: {[depPath: string]: string[]}
    importerModulesDir: string,
    importerPath: string,
    pkg: PackageJson,
    prefix: string,
    shamefullyFlatten: boolean,
  }>,
  include: IncludedDependencies,
  modulesFile: Modules | null,
  pendingBuilds: string[],
  shrinkwrapDirectory: string,
  virtualStoreDir: string,
  skipped: Set<string>,
  storePath: string,
  wantedShrinkwrap: Shrinkwrap,
}

export interface ImportersOptions {
  bin?: string,
  prefix: string,
  shamefullyFlatten?: boolean,
}

export type StrictImportersOptions = ImportersOptions & {
  bin: string,
  prefix: string,
  shamefullyFlatten: boolean,
  importerModulesDir: string,
  importerPath: string,
}

export default async function getContext (
  opts: {
    force: boolean,
    shrinkwrapDirectory: string,
    hooks?: {
      readPackage?: ReadPackageHook,
    },
    include?: IncludedDependencies,
    independentLeaves: boolean,
    importers: StrictImportersOptions[],
    registry: string,
    shrinkwrap: boolean,
    store: string,
  },
  installType?: 'named' | 'general',
): Promise<PnpmContext> {
  const storePath = opts.store

  const virtualStoreDir = await realNodeModulesDir(opts.shrinkwrapDirectory)
  const modules = await readModulesYaml(virtualStoreDir)

  if (modules) {
    await validateNodeModules(modules, opts.importers, {
      force: opts.force && installType === 'general',
      include: opts.include,
      independentLeaves: opts.independentLeaves,
      shrinkwrapDirectory: opts.shrinkwrapDirectory,
      store: opts.store,
    })
  }

  await mkdirp(storePath)

  const ctx: PnpmContext = {
    importers: await Promise.all(
      opts.importers.map(async (importer) => {
        let pkg = await safeReadPkgFromDir(importer.prefix) || {} as PackageJson
        pkg = opts.hooks && opts.hooks.readPackage ? opts.hooks.readPackage(pkg) : pkg
        packageJsonLogger.debug({
          initial: pkg,
          prefix: importer.prefix,
        })
        return {
          bin: importer.bin,
          hoistedAliases: modules && modules.importers[importer.importerPath] && modules.importers[importer.importerPath].hoistedAliases || {},
          importerModulesDir: await realNodeModulesDir(importer.prefix),
          importerPath: importer.importerPath,
          pkg,
          prefix: importer.prefix,
          shamefullyFlatten: Boolean(importer.shamefullyFlatten || modules && modules.importers[importer.importerPath] && modules.importers[importer.importerPath].shamefullyFlatten),
        }
      }),
    ),
    include: opts.include || modules && modules.included || { dependencies: true, devDependencies: true, optionalDependencies: true },
    modulesFile: modules,
    pendingBuilds: modules && modules.pendingBuilds || [],
    shrinkwrapDirectory: opts.shrinkwrapDirectory,
    skipped: new Set(modules && modules.skipped || []),
    storePath,
    virtualStoreDir,
    ...await readShrinkwrapFile({
      force: opts.force,
      importerPaths: opts.importers.map((importer) => importer.importerPath),
      registry: opts.registry,
      shrinkwrap: opts.shrinkwrap,
      shrinkwrapDirectory: opts.shrinkwrapDirectory,
    }),
  }

  return ctx
}

async function validateNodeModules (
  modules: Modules,
  importers: Array<{
    importerModulesDir: string,
    importerPath: string,
    prefix: string,
    shamefullyFlatten: boolean,
  }>,
  opts: {
    force: boolean,
    shrinkwrapDirectory: string,
    include?: IncludedDependencies,
    independentLeaves: boolean,
    store: string,
  },
) {
  if (Boolean(modules.independentLeaves) !== opts.independentLeaves) {
    if (opts.force) {
      await Promise.all(importers.map(async (importer) => {
        logger.info({
          message: `Recreating ${importer.importerModulesDir}`,
          prefix: importer.prefix,
        })
        await removeAllExceptOuterLinks(importer.importerModulesDir)
      }))
      return
    }
    if (modules.independentLeaves) {
      throw new PnpmError(
        'ERR_PNPM_INDEPENDENT_LEAVES_WANTED',
        'This "node_modules" folder was created using the --independent-leaves option.'
        + ' You must add that option, or else run "pnpm install --force" to recreate the "node_modules" folder.',
      )
    }
    throw new PnpmError(
      'ERR_PNPM_INDEPENDENT_LEAVES_NOT_WANTED',
      'This "node_modules" folder was created without the --independent-leaves option.'
      + ' You must remove that option, or else "pnpm install --force" to recreate the "node_modules" folder.',
    )
  }
  // TODO: run it concurrently
  for (const importer of importers) {
    try {
      if (modules.importers && modules.importers[importer.importerPath] && Boolean(modules.importers[importer.importerPath].shamefullyFlatten) !== importer.shamefullyFlatten) {
        if (modules.importers[importer.importerPath].shamefullyFlatten) {
          throw new PnpmError(
            'ERR_PNPM_SHAMEFULLY_FLATTEN_WANTED',
            'This "node_modules" folder was created using the --shamefully-flatten option.'
            + ' You must add this option, or else add the --force option to recreate the "node_modules" folder.',
          )
        }
        throw new PnpmError(
          'ERR_PNPM_SHAMEFULLY_FLATTEN_NOT_WANTED',
          'This "node_modules" folder was created without the --shamefully-flatten option.'
          + ' You must remove that option, or else add the --force option to recreate the "node_modules" folder.',
        )
      }
      checkCompatibility(modules, {storePath: opts.store, modulesPath: importer.importerModulesDir})
      if (opts.shrinkwrapDirectory !== importer.prefix && opts.include && modules.included) {
        for (const depsField of DEPENDENCIES_FIELDS) {
          if (opts.include[depsField] !== modules.included[depsField]) {
            throw new PnpmError('ERR_PNPM_INCLUDED_DEPS_CONFLICT',
              `node_modules (at "${opts.shrinkwrapDirectory}") was installed with ${stringifyIncludedDeps(modules.included)}. ` +
              `Current install wants ${stringifyIncludedDeps(opts.include)}.`,
            )
          }
        }
      }
    } catch (err) {
      if (!opts.force) throw err
      logger.info({
        message: `Recreating ${importer.importerModulesDir}`,
        prefix: importer.prefix,
      })
      await removeAllExceptOuterLinks(importer.importerModulesDir)
    }
  }
}

function stringifyIncludedDeps (included: IncludedDependencies) {
  return DEPENDENCIES_FIELDS.filter((depsField) => included[depsField]).join(', ')
}

export interface PnpmSingleContext {
  currentShrinkwrap: Shrinkwrap,
  existsCurrentShrinkwrap: boolean,
  existsWantedShrinkwrap: boolean,
  hoistedAliases: {[depPath: string]: string[]}
  importerModulesDir: string,
  importerPath: string,
  pkg: PackageJson,
  prefix: string,
  include: IncludedDependencies,
  modulesFile: Modules | null,
  pendingBuilds: string[],
  shrinkwrapDirectory: string,
  virtualStoreDir: string,
  skipped: Set<string>,
  storePath: string,
  wantedShrinkwrap: Shrinkwrap,
}

export async function getContextForSingleImporter (
  opts: {
    force: boolean,
    shrinkwrapDirectory: string,
    hooks?: {
      readPackage?: ReadPackageHook,
    },
    include?: IncludedDependencies,
    independentLeaves: boolean,
    prefix: string,
    registry: string,
    shamefullyFlatten: boolean,
    shrinkwrap: boolean,
    store: string,
  },
): Promise<PnpmSingleContext> {
  const storePath = opts.store

  const shrinkwrapDirectory = opts.shrinkwrapDirectory || opts.prefix
  const virtualStoreDir = await realNodeModulesDir(shrinkwrapDirectory)
  const modules = await readModulesYaml(virtualStoreDir)

  const importerModulesDir = await realNodeModulesDir(opts.prefix)
  const importerPath = getImporterPath(shrinkwrapDirectory, opts.prefix)

  if (modules) {
    const importers = [
      {
        importerModulesDir,
        importerPath,
        prefix: opts.prefix,
        shamefullyFlatten: opts.shamefullyFlatten,
      },
    ]
    validateNodeModules(modules, importers, {
      force: opts.force,
      include: opts.include,
      independentLeaves: opts.independentLeaves,
      shrinkwrapDirectory: opts.shrinkwrapDirectory,
      store: opts.store,
    })
  }

  const files = await Promise.all([
    safeReadPkgFromDir(opts.prefix),
    mkdirp(storePath),
  ])
  const pkg = files[0] || {} as PackageJson
  const ctx: PnpmSingleContext = {
    hoistedAliases: modules && modules.importers[importerPath] && modules.importers[importerPath].hoistedAliases || {},
    importerModulesDir,
    importerPath,
    include: opts.include || modules && modules.included || { dependencies: true, devDependencies: true, optionalDependencies: true },
    modulesFile: modules,
    pendingBuilds: modules && modules.pendingBuilds || [],
    pkg: opts.hooks && opts.hooks.readPackage ? opts.hooks.readPackage(pkg) : pkg,
    prefix: opts.prefix,
    shrinkwrapDirectory,
    skipped: new Set(modules && modules.skipped || []),
    storePath,
    virtualStoreDir,
    ...await readShrinkwrapFile({
      force: opts.force,
      importerPaths: [importerPath],
      registry: opts.registry,
      shrinkwrap: opts.shrinkwrap,
      shrinkwrapDirectory,
    }),
  }
  packageJsonLogger.debug({
    initial: pkg,
    prefix: opts.prefix,
  })

  return ctx
}
