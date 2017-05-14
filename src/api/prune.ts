import path = require('path')
import R = require('ramda')
import getContext from './getContext'
import {PnpmOptions, Package} from '../types'
import extendOptions from './extendOptions'
import getPkgDirs from '../fs/getPkgDirs'
import {fromDir as readPkgFromDir} from '../fs/readPkg'
import lock from './lock'
import removeOrphanPkgs from './removeOrphanPkgs'
import npa = require('npm-package-arg')
import {PackageSpec} from '../resolve'
import {
  ResolvedDependencies,
  prune as pruneShrinkwrap,
} from '../fs/shrinkwrap'

export async function prune(maybeOpts?: PnpmOptions): Promise<void> {
  const opts = extendOptions(maybeOpts)

  const ctx = await getContext(opts)

  if (opts.lock === false) {
    return run()
  }

  return lock(ctx.storePath, run, {stale: opts.lockStaleDuration})

  async function run () {
    if (!ctx.pkg) {
      throw new Error('No package.json found - cannot prune')
    }

    const pkg = ctx.pkg

    const extraneousPkgs = await getExtraneousPkgs(pkg, ctx.root, opts.production)

    const newShr = ctx.shrinkwrap
    newShr.dependencies = <ResolvedDependencies>R.pickBy((value, key) => {
      const spec: PackageSpec = npa(key)
      return extraneousPkgs.indexOf(spec.name) === -1
    }, newShr.dependencies)

    const prunedShr = pruneShrinkwrap(newShr)

    await removeOrphanPkgs(ctx.privateShrinkwrap, prunedShr, ctx.root, ctx.storePath)
  }
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
  const pkgs: Package[] = await Promise.all(pkgDirs.map(readPkgFromDir))
  return pkgs.map(pkg => pkg.name)
}
