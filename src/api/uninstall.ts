import path = require('path')
import * as dp from 'dependency-path'
import R = require('ramda')
import getContext, {PnpmContext} from './getContext'
import getSaveType from '../getSaveType'
import removeDeps from '../removeDeps'
import extendOptions, {
  UninstallOptions,
  StrictUninstallOptions,
} from './extendUninstallOptions'
import lock from './lock'
import {
  write as saveShrinkwrap,
  writeCurrentOnly as saveCurrentShrinkwrapOnly,
  prune as pruneShrinkwrap,
} from 'pnpm-shrinkwrap'
import logger, {streamParser} from '@pnpm/logger'
import {
  save as saveModules,
  LAYOUT_VERSION,
} from '../fs/modulesController'
import removeOrphanPkgs from './removeOrphanPkgs'
import safeIsInnerLink from '../safeIsInnerLink'
import removeTopDependency from '../removeTopDependency'
import shrinkwrapsEqual from './shrinkwrapsEqual'
import {installPkgs} from './install'

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

  const pkgJsonPath = path.join(ctx.root, 'package.json')
  const saveType = getSaveType(opts)
  const pkg = await removeDeps(pkgJsonPath, pkgsToUninstall, saveType)
  const newShr = pruneShrinkwrap(ctx.wantedShrinkwrap, pkg)
  const removedPkgIds = await removeOrphanPkgs({
    oldShrinkwrap: ctx.currentShrinkwrap,
    newShrinkwrap: newShr,
    prefix: ctx.root,
    shamefullyFlatten: opts.shamefullyFlatten,
    storeController: opts.storeController,
    bin: opts.bin,
  })
  ctx.pendingBuilds = ctx.pendingBuilds.filter(pkgId => !removedPkgIds.has(dp.resolve(newShr.registry, pkgId)))
  await opts.storeController.close()
  const currentShrinkwrap = makePartialCurrentShrinkwrap
    ? pruneShrinkwrap(ctx.currentShrinkwrap, pkg)
    : newShr
  if (opts.shrinkwrap) {
    await saveShrinkwrap(ctx.root, newShr, currentShrinkwrap)
  } else {
    await saveCurrentShrinkwrapOnly(ctx.root, currentShrinkwrap)
  }
  await saveModules(path.join(ctx.root, 'node_modules'), {
    packageManager: `${opts.packageManager.name}@${opts.packageManager.version}`,
    store: ctx.storePath,
    skipped: Array.from(ctx.skipped).filter(pkgId => !removedPkgIds.has(pkgId)),
    layoutVersion: LAYOUT_VERSION,
    independentLeaves: opts.independentLeaves,
    pendingBuilds: ctx.pendingBuilds,
    shamefullyFlatten: opts.shamefullyFlatten,
  })
  await removeOuterLinks(pkgsToUninstall, path.join(ctx.root, 'node_modules'), {
    storePath: ctx.storePath,
    bin: opts.bin,
  })

  if (opts.shamefullyFlatten && R.keys(currentShrinkwrap.specifiers).length > 0) {
    await installPkgs(currentShrinkwrap.specifiers, Object.assign({},
      opts, {lock: false, reinstallForFlatten: true, update: false}
    ))
  }

  logger('summary').info()
}

async function removeOuterLinks (
  pkgsToUninstall: string[],
  modules: string,
  opts: {
    storePath: string,
    bin: string,
  }
) {
  // These packages are not in package.json, they were just linked in not installed
  for (const pkgToUninstall of pkgsToUninstall) {
    if (await safeIsInnerLink(modules, pkgToUninstall, opts) !== true) {
      await removeTopDependency({
        name: pkgToUninstall,
        dev: false,
        optional: false,
      }, {
        modules,
        bin: opts.bin,
      })
    }
  }
}
