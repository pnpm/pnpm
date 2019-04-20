import { ENGINE_NAME, LAYOUT_VERSION } from '@pnpm/constants'
import { summaryLogger } from '@pnpm/core-loggers'
import {
  writeCurrentLockfile,
  writeLockfiles,
} from '@pnpm/lockfile-file'
import logger, { streamParser } from '@pnpm/logger'
import { prune } from '@pnpm/modules-cleaner'
import { write as writeModulesYaml } from '@pnpm/modules-yaml'
import { pruneLockfile } from '@pnpm/prune-lockfile'
import { shamefullyFlattenByLockfile } from '@pnpm/shamefully-flatten'
import { ImporterManifest } from '@pnpm/types'
import { getSaveType } from '@pnpm/utils'
import * as dp from 'dependency-path'
import { getContextForSingleImporter, PnpmSingleContext } from '../getContext'
import lock from '../lock'
import lockfilesEqual from '../lockfilesEqual'
import extendOptions, {
  StrictUninstallOptions,
  UninstallOptions,
} from './extendUninstallOptions'
import removeDeps from './removeDeps'

export default async function uninstall (
  manifest: ImporterManifest,
  pkgsToUninstall: string[],
  maybeOpts: UninstallOptions,
) {
  const reporter = maybeOpts && maybeOpts.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }

  const opts = await extendOptions(maybeOpts)

  let newManifest!: ImporterManifest
  if (opts.lock) {
    newManifest = await lock(opts.prefix, _uninstall, {
      locks: opts.locks,
      prefix: opts.prefix,
      stale: opts.lockStaleDuration,
      storeController: opts.storeController,
    })
  } else {
    newManifest = await _uninstall()
  }

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }

  return newManifest

  async function _uninstall () {
    const ctx = await getContextForSingleImporter(manifest, opts)

    if (!ctx.manifest) {
      throw new Error('No package.json found - cannot uninstall')
    }

    return uninstallInContext(pkgsToUninstall, ctx, opts)
  }
}

export async function uninstallInContext (
  pkgsToUninstall: string[],
  ctx: PnpmSingleContext,
  opts: StrictUninstallOptions,
) {
  const makePartialCurrentLockfile = !lockfilesEqual(ctx.currentLockfile, ctx.wantedLockfile)

  const saveType = getSaveType(opts)
  const pkg = await removeDeps(ctx.manifest, pkgsToUninstall, { prefix: opts.prefix, saveType })
  const newLockfile = pruneLockfile(ctx.wantedLockfile, pkg, ctx.importerId, {
    defaultRegistry: ctx.registries.default,
    warn: (message) => logger.warn({ message, prefix: ctx.prefix }),
  })
  const removedPkgIds = await prune({
    importers: [
      {
        bin: opts.bin,
        hoistedAliases: ctx.hoistedAliases,
        id: ctx.importerId,
        modulesDir: ctx.modulesDir,
        prefix: ctx.prefix,
        removePackages: pkgsToUninstall,
        shamefullyFlatten: opts.shamefullyFlatten,
      },
    ],
    lockfileDirectory: opts.lockfileDirectory,
    newLockfile,
    oldLockfile: ctx.currentLockfile,
    registries: ctx.registries,
    storeController: opts.storeController,
    virtualStoreDir: ctx.virtualStoreDir,
  })
  ctx.pendingBuilds = ctx.pendingBuilds.filter((pkgId) => !removedPkgIds.has(dp.resolve(ctx.registries, pkgId)))
  await opts.storeController.close()
  const currentLockfile = makePartialCurrentLockfile
    ? pruneLockfile(ctx.currentLockfile, pkg, ctx.importerId, { defaultRegistry: ctx.registries.default })
    : newLockfile
  const lockfileOpts = { forceSharedFormat: opts.forceSharedLockfile }
  if (opts.useLockfile) {
    await writeLockfiles(ctx.lockfileDirectory, newLockfile, currentLockfile, lockfileOpts)
  } else {
    await writeCurrentLockfile(ctx.lockfileDirectory, currentLockfile, lockfileOpts)
  }

  if (opts.shamefullyFlatten) {
    ctx.hoistedAliases = await shamefullyFlattenByLockfile(currentLockfile, ctx.importerId, {
      getIndependentPackageLocation: opts.independentLeaves
        ? async (packageId: string, packageName: string) => {
          const { directory } = await opts.storeController.getPackageLocation(packageId, packageName, {
            lockfileDirectory: ctx.lockfileDirectory,
            targetEngine: opts.sideEffectsCacheRead && ENGINE_NAME || undefined,
          })
          return directory
        }
        : undefined,
      lockfileDirectory: opts.lockfileDirectory,
      modulesDir: ctx.modulesDir,
      registries: ctx.registries,
      virtualStoreDir: ctx.virtualStoreDir,
    }) || {}
  }
  await writeModulesYaml(ctx.virtualStoreDir, {
    ...ctx.modulesFile,
    importers: {
      ...ctx.modulesFile && ctx.modulesFile.importers,
      [ctx.importerId]: {
        hoistedAliases: ctx.hoistedAliases,
        shamefullyFlatten: opts.shamefullyFlatten,
      },
    },
    included: ctx.include,
    independentLeaves: opts.independentLeaves,
    layoutVersion: LAYOUT_VERSION,
    packageManager: `${opts.packageManager.name}@${opts.packageManager.version}`,
    pendingBuilds: ctx.pendingBuilds,
    registries: ctx.registries,
    skipped: Array.from(ctx.skipped).filter((pkgId) => !removedPkgIds.has(pkgId)),
    store: ctx.storePath,
  })

  summaryLogger.debug({ prefix: opts.prefix })

  return pkg
}
