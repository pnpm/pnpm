import rimraf = require('rimraf-then')
import path = require('path')
import getContext, {PnpmContext} from './getContext'
import getSaveType from '../getSaveType'
import removeDeps from '../removeDeps'
import extendOptions from './extendOptions'
import {PnpmOptions, StrictPnpmOptions, Package} from '../types'
import lock from './lock'
import {
  Shrinkwrap,
  save as saveShrinkwrap,
  prune as pruneShrinkwrap,
} from 'pnpm-lockfile'
import {
  save as saveModules,
  LAYOUT_VERSION,
} from '../fs/modulesController'
import removeOrphanPkgs from './removeOrphanPkgs'
import {PackageSpec} from '../resolve'
import pnpmPkgJson from '../pnpmPkgJson'
import safeIsInnerLink from '../safeIsInnerLink'
import removeTopDependency from '../removeTopDependency'

export default async function uninstallCmd (pkgsToUninstall: string[], maybeOpts?: PnpmOptions) {
  const opts = extendOptions(maybeOpts)
  return lock(opts.prefix, async () => {
    const ctx = await getContext(opts)

    if (!ctx.pkg) {
      throw new Error('No package.json found - cannot uninstall')
    }

    if (opts.lock === false) {
      return run()
    }

    return lock(ctx.storePath, run, {stale: opts.lockStaleDuration})

    function run () {
      return uninstallInContext(pkgsToUninstall, ctx, opts)
    }
  }, {stale: opts.lockStaleDuration})
}

export async function uninstallInContext (pkgsToUninstall: string[], ctx: PnpmContext, opts: StrictPnpmOptions) {
  const pkgJsonPath = path.join(ctx.root, 'package.json')
  const saveType = getSaveType(opts)
  const pkg = await removeDeps(pkgJsonPath, pkgsToUninstall, saveType)
  for (let depName in ctx.shrinkwrap.dependencies) {
    if (!isDependentOn(pkg, depName)) {
      delete ctx.shrinkwrap.dependencies[depName]
      delete ctx.shrinkwrap.specifiers[depName]
    }
  }
  const newShr = await pruneShrinkwrap(ctx.shrinkwrap, pkg)
  const removedPkgIds = await removeOrphanPkgs(ctx.privateShrinkwrap, newShr, ctx.root, ctx.storePath)
  await saveShrinkwrap(ctx.root, newShr)
  await saveModules(path.join(ctx.root, 'node_modules'), {
    packageManager: `${pnpmPkgJson.name}@${pnpmPkgJson.version}`,
    storePath: ctx.storePath,
    skipped: Array.from(ctx.skipped).filter(pkgId => removedPkgIds.indexOf(pkgId) === -1),
    layoutVersion: LAYOUT_VERSION,
    independentLeaves: opts.independentLeaves,
  })
  await removeOuterLinks(pkgsToUninstall, path.join(ctx.root, 'node_modules'), {storePath: ctx.storePath})
}

function isDependentOn (pkg: Package, depName: string): boolean {
  return [
    'dependencies',
    'devDependencies',
    'optionalDependencies',
  ]
  .some(deptype => pkg[deptype] && pkg[deptype][depName])
}

async function removeOuterLinks (
  pkgsToUninstall: string[],
  modules: string,
  opts: {
    storePath: string,
  }
) {
  for (const pkgToUninstall of pkgsToUninstall) {
    if (!await safeIsInnerLink(modules, pkgToUninstall, opts)) {
      await removeTopDependency(pkgToUninstall, modules)
    }
  }
}
