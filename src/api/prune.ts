import path = require('path')
import getContext from './getContext'
import {PnpmOptions, Package} from '../types'
import extendOptions from './extendOptions'
import {uninstallInContext} from './uninstall'
import getPkgDirs from '../fs/getPkgDirs'
import readPkg from '../fs/readPkg'
import lock from './lock'

export async function prune(maybeOpts?: PnpmOptions): Promise<void> {
  const opts = extendOptions(maybeOpts)

  const ctx = await getContext(opts)

  return lock(ctx.storePath, async function () {
    if (!ctx.pkg) {
      throw new Error('No package.json found - cannot prune')
    }

    const pkg = ctx.pkg

    const extraneousPkgs = await getExtraneousPkgs(pkg, ctx.root, opts.production)

    await uninstallInContext(extraneousPkgs, ctx.pkg, ctx, opts)
  },
  {stale: opts.lockStaleDuration})
}

export async function prunePkgs(pkgsToPrune: string[], maybeOpts?: PnpmOptions): Promise<void> {
  const opts = extendOptions(maybeOpts)

  const ctx = await getContext(opts)

  return lock(ctx.storePath, async function () {
    if (!ctx.pkg) {
      throw new Error('No package.json found - cannot prune')
    }
    const pkg = ctx.pkg

    const extraneousPkgs = await getExtraneousPkgs(pkg, ctx.root, opts.production)

    const notPrunable = pkgsToPrune.filter(pkgToPrune => extraneousPkgs.indexOf(pkgToPrune) === -1)
    if (notPrunable.length) {
      const err = new Error(`Unable to prune ${notPrunable.join(', ')} because it is not an extraneous package`)
      err['code'] = 'PRUNE_NOT_EXTR'
      throw err
    }

    await uninstallInContext(pkgsToPrune, ctx.pkg, ctx, opts)
  },
  {stale: opts.lockStaleDuration})
}

async function getExtraneousPkgs (pkg: Package, root: string, production: boolean) {
  const saveTypes = getSaveTypes(production)
  const savedDepsMap = saveTypes.reduce((allDeps, deps) => Object.assign({}, allDeps, pkg[deps]), {})
  const savedDeps = Object.keys(savedDepsMap)
  const modules = path.join(root, 'node_modules')
  const pkgsInFS = await getPkgsInFS(modules)
  const extraneousPkgs = pkgsInFS.filter((pkgInFS: string) => savedDeps.indexOf(pkgInFS) === -1)
  return extraneousPkgs
}

const prodDepTypes = ['dependencies', 'optionalDependencies']
const devOnlyDepTypes = ['devDependencies']

function getSaveTypes (production: boolean) {
  if (production) {
    return prodDepTypes
  }
  return prodDepTypes.concat(devOnlyDepTypes)
}

async function getPkgsInFS (modules: string): Promise<string[]> {
  const pkgDirs = await getPkgDirs(modules)
  const pkgs: Package[] = await Promise.all(pkgDirs.map(readPkg))
  return pkgs.map(pkg => pkg.name)
}
