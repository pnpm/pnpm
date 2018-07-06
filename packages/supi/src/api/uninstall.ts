import logger, {streamParser} from '@pnpm/logger'
import {write as writeModulesYaml} from '@pnpm/modules-yaml'
import {
  getSaveType,
  removeOrphanPackages as removeOrphanPkgs,
  removeTopDependency,
} from '@pnpm/utils'
import * as dp from 'dependency-path'
import path = require('path')
import {
  prune as pruneShrinkwrap,
  write as saveShrinkwrap,
  writeCurrentOnly as saveCurrentShrinkwrapOnly,
} from 'pnpm-shrinkwrap'
import R = require('ramda')
import {LAYOUT_VERSION} from '../constants'
import removeDeps from '../removeDeps'
import safeIsInnerLink from '../safeIsInnerLink'
import extendOptions, {
  StrictUninstallOptions,
  UninstallOptions,
} from './extendUninstallOptions'
import getContext, {PnpmContext} from './getContext'
import {installPkgs} from './install'
import lock from './lock'
import shrinkwrapsEqual from './shrinkwrapsEqual'

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
    await lock(opts.prefix, _uninstall, {stale: opts.lockStaleDuration, locks: opts.locks})
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
  const newShr = pruneShrinkwrap(ctx.wantedShrinkwrap, pkg)
  const removedPkgIds = await removeOrphanPkgs({
    bin: opts.bin,
    hoistedAliases: ctx.hoistedAliases,
    newShrinkwrap: newShr,
    oldShrinkwrap: ctx.currentShrinkwrap,
    prefix: ctx.prefix,
    shamefullyFlatten: opts.shamefullyFlatten,
    storeController: opts.storeController,
  })
  ctx.pendingBuilds = ctx.pendingBuilds.filter((pkgId) => !removedPkgIds.has(dp.resolve(newShr.registry, pkgId)))
  await opts.storeController.close()
  const currentShrinkwrap = makePartialCurrentShrinkwrap
    ? pruneShrinkwrap(ctx.currentShrinkwrap, pkg)
    : newShr
  if (opts.shrinkwrap) {
    await saveShrinkwrap(ctx.prefix, newShr, currentShrinkwrap)
  } else {
    await saveCurrentShrinkwrapOnly(ctx.prefix, currentShrinkwrap)
  }
  await writeModulesYaml(path.join(ctx.prefix, 'node_modules'), {
    hoistedAliases: ctx.hoistedAliases,
    independentLeaves: opts.independentLeaves,
    layoutVersion: LAYOUT_VERSION,
    packageManager: `${opts.packageManager.name}@${opts.packageManager.version}`,
    pendingBuilds: ctx.pendingBuilds,
    shamefullyFlatten: opts.shamefullyFlatten,
    skipped: Array.from(ctx.skipped).filter((pkgId) => !removedPkgIds.has(pkgId)),
    store: ctx.storePath,
  })
  await removeOuterLinks(pkgsToUninstall, path.join(ctx.prefix, 'node_modules'), {
    bin: opts.bin,
    prefix: opts.prefix,
    storePath: ctx.storePath,
  })

  if (opts.shamefullyFlatten) {
    await installPkgs(currentShrinkwrap.specifiers, {...opts, lock: false, reinstallForFlatten: true, update: false})
  }

  logger('summary').info()
}

async function removeOuterLinks (
  pkgsToUninstall: string[],
  modules: string,
  opts: {
    bin: string,
    storePath: string,
    prefix: string,
  },
) {
  // These packages are not in package.json, they were just linked in not installed
  for (const pkgToUninstall of pkgsToUninstall) {
    if (await safeIsInnerLink(modules, pkgToUninstall, opts) !== true) {
      await removeTopDependency({
        dev: false,
        name: pkgToUninstall,
        optional: false,
      }, {
        bin: opts.bin,
        modules,
        prefix: opts.prefix,
      })
    }
  }
}
