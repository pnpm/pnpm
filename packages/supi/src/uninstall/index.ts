import { summaryLogger } from '@pnpm/core-loggers'
import logger, { streamParser } from '@pnpm/logger'
import { prune } from '@pnpm/modules-cleaner'
import { write as writeModulesYaml } from '@pnpm/modules-yaml'
import { prune as pruneShrinkwrap } from '@pnpm/prune-shrinkwrap'
import { shamefullyFlattenByShrinkwrap } from '@pnpm/shamefully-flatten'
import {
  write as saveShrinkwrap,
  writeCurrentOnly as saveCurrentShrinkwrapOnly,
} from '@pnpm/shrinkwrap-file'
import { getSaveType } from '@pnpm/utils'
import * as dp from 'dependency-path'
import path = require('path')
import { LAYOUT_VERSION } from '../constants'
import { getContextForSingleImporter, PnpmSingleContext } from '../getContext'
import lock from '../lock'
import shrinkwrapsEqual from '../shrinkwrapsEqual'
import extendOptions, {
  StrictUninstallOptions,
  UninstallOptions,
} from './extendUninstallOptions'
import removeDeps from './removeDeps'

export default async function uninstall (
  pkgsToUninstall: string[],
  maybeOpts: UninstallOptions,
) {
  const reporter = maybeOpts && maybeOpts.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }

  const opts = await extendOptions(maybeOpts)

  if (opts.lock) {
    await lock(opts.prefix, _uninstall, {
      locks: opts.locks,
      prefix: opts.prefix,
      stale: opts.lockStaleDuration,
      storeController: opts.storeController,
    })
  } else {
    await _uninstall()
  }

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }

  async function _uninstall () {
    const ctx = await getContextForSingleImporter(opts)

    if (!ctx.pkg) {
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
  const makePartialCurrentShrinkwrap = !shrinkwrapsEqual(ctx.currentShrinkwrap, ctx.wantedShrinkwrap)

  const pkgJsonPath = path.join(ctx.prefix, 'package.json')
  const saveType = getSaveType(opts)
  const pkg = await removeDeps(pkgJsonPath, pkgsToUninstall, { prefix: opts.prefix, saveType })
  const newShr = pruneShrinkwrap(ctx.wantedShrinkwrap, pkg, ctx.importerId, {
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
    newShrinkwrap: newShr,
    oldShrinkwrap: ctx.currentShrinkwrap,
    registries: ctx.registries,
    shrinkwrapDirectory: opts.shrinkwrapDirectory,
    storeController: opts.storeController,
    virtualStoreDir: ctx.virtualStoreDir,
  })
  ctx.pendingBuilds = ctx.pendingBuilds.filter((pkgId) => !removedPkgIds.has(dp.resolve(ctx.registries.default, pkgId)))
  await opts.storeController.close()
  const currentShrinkwrap = makePartialCurrentShrinkwrap
    ? pruneShrinkwrap(ctx.currentShrinkwrap, pkg, ctx.importerId, { defaultRegistry: ctx.registries.default })
    : newShr
  const shrinkwrapOpts = { forceSharedFormat: opts.forceSharedShrinkwrap }
  if (opts.shrinkwrap) {
    await saveShrinkwrap(ctx.shrinkwrapDirectory, newShr, currentShrinkwrap, shrinkwrapOpts)
  } else {
    await saveCurrentShrinkwrapOnly(ctx.shrinkwrapDirectory, currentShrinkwrap, shrinkwrapOpts)
  }

  if (opts.shamefullyFlatten) {
    ctx.hoistedAliases = await shamefullyFlattenByShrinkwrap(currentShrinkwrap, ctx.importerId, {
      defaultRegistry: ctx.registries.default,
      modulesDir: ctx.modulesDir,
      prefix: opts.prefix,
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
}
