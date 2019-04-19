import { packageJsonLogger } from '@pnpm/core-loggers'
import { Lockfile } from '@pnpm/lockfile-file'
import logger from '@pnpm/logger'
import {
  IncludedDependencies,
  Modules,
} from '@pnpm/modules-yaml'
import readManifests from '@pnpm/read-manifests'
import {
  DEPENDENCIES_FIELDS,
  PackageJson,
  ReadPackageHook,
  Registries,
} from '@pnpm/types'
import {
  safeReadPackageFromDir as safeReadPkgFromDir,
} from '@pnpm/utils'
import mkdirp = require('mkdirp-promise')
import removeAllExceptOuterLinks = require('remove-all-except-outer-links')
import { PnpmError } from '../errorTypes'
import checkCompatibility from './checkCompatibility'
import readLockfileFile from './readLockfiles'

export interface PnpmContext<T> {
  currentLockfile: Lockfile,
  existsCurrentLockfile: boolean,
  existsWantedLockfile: boolean,
  importers: Array<{
    bin: string,
    hoistedAliases: {[depPath: string]: string[]}
    modulesDir: string,
    id: string,
    pkg: PackageJson,
    prefix: string,
    shamefullyFlatten: boolean,
  } & T>,
  include: IncludedDependencies,
  modulesFile: Modules | null,
  pendingBuilds: string[],
  lockfileDirectory: string,
  virtualStoreDir: string,
  skipped: Set<string>,
  storePath: string,
  wantedLockfile: Lockfile,
  registries: Registries,
}

export interface ImportersOptions {
  bin?: string,
  pkg: PackageJson,
  prefix: string,
  shamefullyFlatten?: boolean,
}

export default async function getContext<T> (
  importers: (ImportersOptions & T)[],
  opts: {
    force: boolean,
    forceSharedLockfile: boolean,
    lockfileDirectory: string,
    hooks?: {
      readPackage?: ReadPackageHook,
    },
    include?: IncludedDependencies,
    independentLeaves: boolean,
    registries: Registries,
    shamefullyFlatten: boolean,
    store: string,
    useLockfile: boolean,
  },
): Promise<PnpmContext<T>> {
  const manifests = await readManifests(importers, opts.lockfileDirectory, {
    shamefullyFlatten: opts.shamefullyFlatten,
  })

  if (manifests.modules) {
    await validateNodeModules(manifests.modules, manifests.importers, {
      force: opts.force,
      include: opts.include,
      independentLeaves: opts.independentLeaves,
      lockfileDirectory: opts.lockfileDirectory,
      store: opts.store,
    })
  }

  await mkdirp(opts.store)

  importers.forEach((importer) => {
    packageJsonLogger.debug({
      initial: importer.pkg,
      prefix: importer.prefix,
    })
  })
  if (opts.hooks && opts.hooks.readPackage) {
    importers = importers.map((importer) => ({
      ...importer,
      pkg: opts.hooks!.readPackage!(importer.pkg),
    }))
  }

  const importerOptionsByPrefix = importers.reduce((prev, curr) => {
    prev[curr.prefix] = curr
    return prev
  }, {})
  const ctx: PnpmContext<T> = {
    importers: manifests.importers.map((importer) => ({
      ...importerOptionsByPrefix[importer.prefix],
      ...importer,
    })),
    include: opts.include || manifests.include,
    lockfileDirectory: opts.lockfileDirectory,
    modulesFile: manifests.modules,
    pendingBuilds: manifests.pendingBuilds,
    registries: {
      ...opts.registries,
      ...manifests.registries,
    },
    skipped: manifests.skipped,
    storePath: opts.store,
    virtualStoreDir: manifests.virtualStoreDir,
    ...await readLockfileFile({
      force: opts.force,
      forceSharedLockfile: opts.forceSharedLockfile,
      importers: manifests.importers,
      lockfileDirectory: opts.lockfileDirectory,
      registry: opts.registries.default,
      useLockfile: opts.useLockfile,
    }),
  }

  return ctx
}

async function validateNodeModules (
  modules: Modules,
  importers: Array<{
    modulesDir: string,
    id: string,
    prefix: string,
    currentShamefullyFlatten: boolean | null,
    shamefullyFlatten: boolean,
  }>,
  opts: {
    force: boolean,
    lockfileDirectory: string,
    include?: IncludedDependencies,
    independentLeaves: boolean,
    store: string,
  },
) {
  if (Boolean(modules.independentLeaves) !== opts.independentLeaves) {
    if (opts.force) {
      await Promise.all(importers.map(async (importer) => {
        logger.info({
          message: `Recreating ${importer.modulesDir}`,
          prefix: importer.prefix,
        })
        try {
          await removeAllExceptOuterLinks(importer.modulesDir)
        } catch (err) {
          if (err.code !== 'ENOENT') throw err
        }
      }))
      // TODO: remove the node_modules in the lockfile directory
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
  await Promise.all(importers.map(async (importer) => {
    try {
      if (typeof importer.currentShamefullyFlatten === 'boolean' && importer.currentShamefullyFlatten !== importer.shamefullyFlatten) {
        if (importer.currentShamefullyFlatten) {
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
      checkCompatibility(modules, { storePath: opts.store, modulesPath: importer.modulesDir })
      if (opts.lockfileDirectory !== importer.prefix && opts.include && modules.included) {
        for (const depsField of DEPENDENCIES_FIELDS) {
          if (opts.include[depsField] !== modules.included[depsField]) {
            throw new PnpmError('ERR_PNPM_INCLUDED_DEPS_CONFLICT',
              `node_modules (at "${opts.lockfileDirectory}") was installed with ${stringifyIncludedDeps(modules.included)}. ` +
              `Current install wants ${stringifyIncludedDeps(opts.include)}.`,
            )
          }
        }
      }
    } catch (err) {
      if (!opts.force) throw err
      logger.info({
        message: `Recreating ${importer.modulesDir}`,
        prefix: importer.prefix,
      })
      try {
        await removeAllExceptOuterLinks(importer.modulesDir)
      } catch (err) {
        if (err.code !== 'ENOENT') throw err
      }
    }
  }))
}

function stringifyIncludedDeps (included: IncludedDependencies) {
  return DEPENDENCIES_FIELDS.filter((depsField) => included[depsField]).join(', ')
}

export interface PnpmSingleContext {
  currentLockfile: Lockfile,
  existsCurrentLockfile: boolean,
  existsWantedLockfile: boolean,
  hoistedAliases: {[depPath: string]: string[]}
  modulesDir: string,
  importerId: string,
  pkg: PackageJson,
  prefix: string,
  include: IncludedDependencies,
  modulesFile: Modules | null,
  pendingBuilds: string[],
  registries: Registries,
  lockfileDirectory: string,
  virtualStoreDir: string,
  skipped: Set<string>,
  storePath: string,
  wantedLockfile: Lockfile,
}

export async function getContextForSingleImporter (
  pkg: PackageJson,
  opts: {
    force: boolean,
    forceSharedLockfile: boolean,
    lockfileDirectory: string,
    hooks?: {
      readPackage?: ReadPackageHook,
    },
    include?: IncludedDependencies,
    independentLeaves: boolean,
    prefix: string,
    registries: Registries,
    shamefullyFlatten: boolean,
    store: string,
    useLockfile: boolean,
  },
): Promise<PnpmSingleContext> {
  const manifests = await readManifests(
    [
      {
        prefix: opts.prefix,
      },
    ],
    opts.lockfileDirectory,
    {
      shamefullyFlatten: opts.shamefullyFlatten,
    },
  )

  const storePath = opts.store

  const importer = manifests.importers[0]
  const modulesDir = importer.modulesDir
  const importerId = importer.id

  if (manifests.modules) {
    await validateNodeModules(manifests.modules, manifests.importers, {
      force: opts.force,
      include: opts.include,
      independentLeaves: opts.independentLeaves,
      lockfileDirectory: opts.lockfileDirectory,
      store: opts.store,
    })
  }

  await mkdirp(storePath)
  const ctx: PnpmSingleContext = {
    hoistedAliases: importer.hoistedAliases,
    importerId,
    include: opts.include || manifests.include,
    lockfileDirectory: opts.lockfileDirectory,
    modulesDir,
    modulesFile: manifests.modules,
    pendingBuilds: manifests.pendingBuilds,
    pkg: opts.hooks && opts.hooks.readPackage ? opts.hooks.readPackage(pkg) : pkg,
    prefix: opts.prefix,
    registries: {
      ...opts.registries,
      ...manifests.registries,
    },
    skipped: manifests.skipped,
    storePath,
    virtualStoreDir: manifests.virtualStoreDir,
    ...await readLockfileFile({
      force: opts.force,
      forceSharedLockfile: opts.forceSharedLockfile,
      importers: [{ id: importerId, prefix: opts.prefix }],
      lockfileDirectory: opts.lockfileDirectory,
      registry: opts.registries.default,
      useLockfile: opts.useLockfile,
    }),
  }
  packageJsonLogger.debug({
    initial: pkg,
    prefix: opts.prefix,
  })

  return ctx
}
