import { summaryLogger } from '@pnpm/core-loggers'
import logger, { streamParser } from '@pnpm/logger'
import { prune, removeDirectDependency } from '@pnpm/modules-cleaner'
import { write as writeModulesYaml } from '@pnpm/modules-yaml'
import { getSaveType } from '@pnpm/utils'
import * as dp from 'dependency-path'
import path = require('path')
import {
  prune as pruneShrinkwrap,
  write as saveShrinkwrap,
  writeCurrentOnly as saveCurrentShrinkwrapOnly,
} from 'pnpm-shrinkwrap'
import { LAYOUT_VERSION } from '../constants'
import getContext, { PnpmContext } from '../getContext'
import lock from '../lock'
import safeIsInnerLink from '../safeIsInnerLink'
import { shamefullyFlattenGraphByShrinkwrap } from '../shamefullyFlattenGraph'
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
    })
  } else {
    await _uninstall()
  }

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }

  async function _uninstall () {
    const ctx = await getContext(opts)

    if (!ctx.pkg) {
      throw new Error('No package.json found - cannot uninstall')
    }

    return uninstallInContext(pkgsToUninstall, ctx, opts)
  }
}

export async function uninstallInContext (
  pkgsToUninstall: string[],
  ctx: PnpmContext,
  opts: StrictUninstallOptions,
) {
  const makePartialCurrentShrinkwrap = !shrinkwrapsEqual(ctx.currentShrinkwrap, ctx.wantedShrinkwrap)

  const pkgJsonPath = path.join(ctx.prefix, 'package.json')
  const saveType = getSaveType(opts)
  const pkg = await removeDeps(pkgJsonPath, pkgsToUninstall, { prefix: opts.prefix, saveType })
  const newShr = pruneShrinkwrap(ctx.wantedShrinkwrap, pkg, ctx.importerPath, (message) => logger.warn({message, prefix: ctx.prefix}))
  const removedPkgIds = await prune({
    importers: [
      {
        bin: opts.bin,
        hoistedAliases: ctx.hoistedAliases,
        importerModulesDir: ctx.importerModulesDir,
        importerPath: ctx.importerPath,
        prefix: ctx.prefix,
        shamefullyFlatten: opts.shamefullyFlatten,
      },
    ],
    newShrinkwrap: newShr,
    oldShrinkwrap: ctx.currentShrinkwrap,
    storeController: opts.storeController,
    virtualStoreDir: ctx.virtualStoreDir,
  })
  ctx.pendingBuilds = ctx.pendingBuilds.filter((pkgId) => !removedPkgIds.has(dp.resolve(newShr.registry, pkgId)))
  await opts.storeController.close()
  const currentShrinkwrap = makePartialCurrentShrinkwrap
    ? pruneShrinkwrap(ctx.currentShrinkwrap, pkg, ctx.importerPath)
    : newShr
  if (opts.shrinkwrap) {
    await saveShrinkwrap(ctx.shrinkwrapDirectory, newShr, currentShrinkwrap)
  } else {
    await saveCurrentShrinkwrapOnly(ctx.shrinkwrapDirectory, currentShrinkwrap)
  }
  await removeOuterLinks(pkgsToUninstall, ctx.importerModulesDir, {
    bin: opts.bin,
    prefix: opts.prefix,
    storePath: ctx.storePath,
  })

  if (opts.shamefullyFlatten) {
    ctx.hoistedAliases = await shamefullyFlattenGraphByShrinkwrap(currentShrinkwrap, ctx.importerPath, {
      importerModulesDir: ctx.importerModulesDir,
      prefix: opts.prefix,
      virtualStoreDir: ctx.virtualStoreDir,
    }) || {}
  }
  await writeModulesYaml(ctx.virtualStoreDir, {
    ...ctx.modulesFile,
    importers: {
      ...ctx.modulesFile && ctx.modulesFile.importers,
      [ctx.importerPath]: {
        hoistedAliases: ctx.hoistedAliases,
        shamefullyFlatten: opts.shamefullyFlatten,
      },
    },
    included: ctx.include,
    independentLeaves: opts.independentLeaves,
    layoutVersion: LAYOUT_VERSION,
    packageManager: `${opts.packageManager.name}@${opts.packageManager.version}`,
    pendingBuilds: ctx.pendingBuilds,
    skipped: Array.from(ctx.skipped).filter((pkgId) => !removedPkgIds.has(pkgId)),
    store: ctx.storePath,
  })

  summaryLogger.debug({prefix: opts.prefix})
}

async function removeOuterLinks (
  pkgsToUninstall: string[],
  importerModulesDir: string,
  opts: {
    bin: string,
    storePath: string,
    prefix: string,
  },
) {
  const safeIsInnerLinkOpts = {
    hideAlienModules: true,
    prefix: opts.prefix,
    storePath: opts.storePath,
  }
  // These packages are not in package.json, they were just linked in not installed
  for (const pkgToUninstall of pkgsToUninstall) {
    if (await safeIsInnerLink(importerModulesDir, pkgToUninstall, safeIsInnerLinkOpts) !== true) {
      await removeDirectDependency({
        dev: false,
        name: pkgToUninstall,
        optional: false,
      }, {
        bin: opts.bin,
        importerModulesDir,
        prefix: opts.prefix,
      })
    }
  }
}
