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
  hoistedAliases: {[depPath: string]: string[]}
  importerModulesDir: string,
  importerPath: string,
  include: IncludedDependencies,
  modulesFile: Modules | null,
  pendingBuilds: string[],
  pkg: PackageJson,
  prefix: string,
  shrinkwrapDirectory: string,
  virtualStoreDir: string,
  skipped: Set<string>,
  storePath: string,
  wantedShrinkwrap: Shrinkwrap,
}

export default async function getContext (
  opts: {
    force: boolean,
    shrinkwrapDirectory?: string,
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
  installType?: 'named' | 'general',
): Promise<PnpmContext> {
  const storePath = opts.store

  const shrinkwrapDirectory = opts.shrinkwrapDirectory || opts.prefix
  const virtualStoreDir = await realNodeModulesDir(shrinkwrapDirectory)
  const modules = await readModulesYaml(virtualStoreDir)

  const importerModulesDir = await realNodeModulesDir(opts.prefix)
  const importerPath = getImporterPath(shrinkwrapDirectory, opts.prefix)

  if (modules) {
    try {
      if (Boolean(modules.independentLeaves) !== opts.independentLeaves) {
        if (modules.independentLeaves) {
          throw new PnpmError(
            'ERR_PNPM_INDEPENDENT_LEAVES_WANTED',
            'This "node_modules" folder was created using the --independent-leaves option.'
            + ' You must add that option, or else add the --force option to recreate the "node_modules" folder.',
          )
        }
        throw new PnpmError(
          'ERR_PNPM_INDEPENDENT_LEAVES_NOT_WANTED',
          'This "node_modules" folder was created without the --independent-leaves option.'
          + ' You must remove that option, or else add the --force option to recreate the "node_modules" folder.',
        )
      }
      if (modules.importers && modules.importers[importerPath] && Boolean(modules.importers[importerPath].shamefullyFlatten) !== opts.shamefullyFlatten) {
        if (modules.importers[importerPath].shamefullyFlatten) {
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
      checkCompatibility(modules, {storePath, modulesPath: importerModulesDir})
      if (shrinkwrapDirectory !== opts.prefix && opts.include && modules.included) {
        for (const depsField of DEPENDENCIES_FIELDS) {
          if (opts.include[depsField] !== modules.included[depsField]) {
            throw new PnpmError('ERR_PNPM_INCLUDED_DEPS_CONFLICT',
              `node_modules (at "${shrinkwrapDirectory}") was installed with ${stringifyIncludedDeps(modules.included)}. ` +
              `Current install wants ${stringifyIncludedDeps(opts.include)}.`,
            )
          }
        }
      }
    } catch (err) {
      if (!opts.force) throw err
      if (installType !== 'general') {
        throw new Error('Named installation cannot be used to regenerate the node_modules structure. Run pnpm install --force')
      }
      logger.info({
        message: `Recreating ${importerModulesDir}`,
        prefix: opts.prefix,
      })
      await removeAllExceptOuterLinks(importerModulesDir)
      return getContext(opts)
    }
  }

  const files = await Promise.all([
    safeReadPkgFromDir(opts.prefix),
    mkdirp(storePath),
  ])
  const pkg = files[0] || {} as PackageJson
  const ctx: PnpmContext = {
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
      importerPath,
      registry: opts.registry,
      shrinkwrap: opts.shrinkwrap,
      shrinkwrapDirectory,
    }),
  }
  packageJsonLogger.debug({
    initial: ctx.pkg,
    prefix: opts.prefix,
  })

  return ctx
}

function stringifyIncludedDeps (included: IncludedDependencies) {
  return DEPENDENCIES_FIELDS.filter((depsField) => included[depsField]).join(', ')
}
