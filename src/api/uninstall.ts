import rimraf = require('rimraf-then')
import path = require('path')
import getContext, {PnpmContext} from './getContext'
import getSaveType from '../getSaveType'
import removeDeps from '../removeDeps'
import extendOptions from './extendOptions'
import readPkg from '../fs/readPkg'
import {PnpmOptions, StrictPnpmOptions, Package} from '../types'
import lock from './lock'
import {
  Shrinkwrap,
  save as saveShrinkwrap,
  prune as pruneShrinkwrap,
} from '../fs/shrinkwrap'
import {
  save as saveModules
} from '../fs/modulesController'
import removeOrphanPkgs from './removeOrphanPkgs'
import npa = require('npm-package-arg')
import {PackageSpec} from '../resolve'
import pnpmPkgJson from '../pnpmPkgJson'
import safeIsInnerLink from '../safeIsInnerLink'
import removeTopDependency from '../removeTopDependency'

export default async function uninstallCmd (pkgsToUninstall: string[], maybeOpts?: PnpmOptions) {
  const opts = extendOptions(maybeOpts)

  const ctx = await getContext(opts)

  if (!ctx.pkg) {
    throw new Error('No package.json found - cannot uninstall')
  }

  const pkg = ctx.pkg
  return lock(
    ctx.storePath,
    () => uninstallInContext(pkgsToUninstall, pkg, ctx, opts),
    {stale: opts.lockStaleDuration}
  )
}

export async function uninstallInContext (pkgsToUninstall: string[], pkg: Package, ctx: PnpmContext, opts: StrictPnpmOptions) {
  const saveType = getSaveType(opts)
  if (saveType) {
    const pkgJsonPath = path.join(ctx.root, 'package.json')
    const pkg = await removeDeps(pkgJsonPath, pkgsToUninstall, saveType)
    for (let depSpecRaw in ctx.shrinkwrap.dependencies) {
      const depSpec: PackageSpec = npa(depSpecRaw)
      if (!isDependentOn(pkg, depSpec.name)) {
        delete ctx.shrinkwrap.dependencies[depSpecRaw]
      }
    }
    const newShr = await pruneShrinkwrap(ctx.shrinkwrap)
    const removedPkgIds = await removeOrphanPkgs(ctx.privateShrinkwrap, newShr, ctx.root, ctx.storePath)
    await saveShrinkwrap(ctx.root, newShr)
    await saveModules(path.join(ctx.root, 'node_modules'), {
      packageManager: `${pnpmPkgJson.name}@${pnpmPkgJson.version}`,
      storePath: ctx.storePath,
      skipped: ctx.skipped.filter(pkgId => removedPkgIds.indexOf(pkgId) === -1),
    })
    await removeOuterLinks(pkgsToUninstall, path.join(ctx.root, 'node_modules'))
  }
}

function isDependentOn (pkg: Package, depName: string): boolean {
  return [
    'dependencies',
    'devDependencies',
    'optionalDependencies',
  ]
  .some(deptype => pkg[deptype] && pkg[deptype][depName])
}

async function removeOuterLinks (pkgsToUninstall: string[], modules: string) {
  for (const pkgToUninstall of pkgsToUninstall) {
    if (!await safeIsInnerLink(modules, pkgToUninstall)) {
      await removeTopDependency(pkgToUninstall, modules)
    }
  }
}
