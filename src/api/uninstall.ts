import rimraf = require('rimraf-then')
import path = require('path')
import getContext, {PnpmContext} from './getContext'
import getSaveType from '../getSaveType'
import removeDeps from '../removeDeps'
import binify from '../binify'
import extendOptions from './extendOptions'
import readPkg from '../fs/readPkg'
import {PnpmOptions, StrictPnpmOptions, Package} from '../types'
import lock from './lock'
import {save as saveGraph, Graph} from '../fs/graphController'

export default async function uninstallCmd (pkgsToUninstall: string[], maybeOpts?: PnpmOptions) {
  const opts = extendOptions(maybeOpts)

  const ctx = await getContext(opts)

  if (!ctx.pkg) {
    throw new Error('No package.json found - cannot uninstall')
  }

  const pkg = ctx.pkg
  return lock(ctx.storePath, () => uninstallInContext(pkgsToUninstall, pkg, ctx, opts))
}

export async function uninstallInContext (pkgsToUninstall: string[], pkg: Package, ctx: PnpmContext, opts: StrictPnpmOptions) {
  pkg.dependencies = pkg.dependencies || {}

  // this is OK. The store might not have records for the package
  // maybe it was cloned, `pnpm install` was not executed
  // and remove is done on a package with no dependencies installed
  ctx.graph[ctx.root] = ctx.graph[ctx.root] || {}
  ctx.graph[ctx.root].dependencies = ctx.graph[ctx.root].dependencies || {}

  const pkgIds = <string[]>pkgsToUninstall
    .map(dep => ctx.graph[ctx.root].dependencies[dep])
    .filter(pkgId => !!pkgId)
  const uninstalledPkgs = tryUninstall(pkgIds.slice(), ctx.graph, ctx.root)
  await Promise.all(
    uninstalledPkgs.map(uninstalledPkg => removeBins(uninstalledPkg, ctx.storePath, ctx.root))
  )
  if (ctx.graph[ctx.root].dependencies) {
    pkgsToUninstall.forEach(dep => {
      delete ctx.graph[ctx.root].dependencies[dep]
    })
    if (!Object.keys(ctx.graph[ctx.root].dependencies).length) {
      delete ctx.graph[ctx.root].dependencies
    }
  }
  await Promise.all(uninstalledPkgs.map(pkgId => removePkgFromStore(pkgId, ctx.storePath)))

  await saveGraph(path.join(ctx.root, 'node_modules'), ctx.graph)
  await Promise.all(pkgsToUninstall.map(dep => rimraf(path.join(ctx.root, 'node_modules', dep))))

  const saveType = getSaveType(opts)
  if (saveType) {
    const pkgJsonPath = path.join(ctx.root, 'package.json')
    await removeDeps(pkgJsonPath, pkgsToUninstall, saveType)
  }
}

function canBeUninstalled (pkgId: string, graph: Graph, pkgPath: string) {
  return !graph[pkgId] || !graph[pkgId].dependents || !graph[pkgId].dependents.length ||
    graph[pkgId].dependents.length === 1 && graph[pkgId].dependents.indexOf(pkgPath) !== -1
}

export function tryUninstall (pkgIds: string[], graph: Graph, pkgPath: string) {
  const uninstalledPkgs: string[] = []
  let numberOfUninstalls: number
  do {
    numberOfUninstalls = 0
    for (let i = 0; i < pkgIds.length; ) {
      if (canBeUninstalled(pkgIds[i], graph, pkgPath)) {
        const uninstalledPkg = pkgIds.splice(i, 1)[0]
        uninstalledPkgs.push(uninstalledPkg)
        const deps = graph[uninstalledPkg] && graph[uninstalledPkg].dependencies || {}
        const depIds = Object.keys(deps).map(depName => deps[depName])
        delete graph[uninstalledPkg]
        depIds.forEach((dep: string) => removeDependency(dep, uninstalledPkg, graph))
        Array.prototype.push.apply(uninstalledPkgs, tryUninstall(depIds, graph, uninstalledPkg))
        numberOfUninstalls++
        continue
      }
      i++
    }
  } while (numberOfUninstalls)
  return uninstalledPkgs
}

function removeDependency (dependentPkgName: string, uninstalledPkg: string, graph: Graph) {
  if (!graph[dependentPkgName].dependents) return
  graph[dependentPkgName].dependents.splice(graph[dependentPkgName].dependents.indexOf(uninstalledPkg), 1)
  if (!graph[dependentPkgName].dependents.length) {
    delete graph[dependentPkgName].dependents
  }
}

async function removeBins (uninstalledPkg: string, store: string, root: string) {
  const uninstalledPkgJson = await readPkg(path.join(store, uninstalledPkg))
  const bins = binify(uninstalledPkgJson)
  return Promise.all(
    Object.keys(bins).map(bin => rimraf(path.join(root, 'node_modules/.bin', bin)))
  )
}

export function removePkgFromStore (pkgId: string, store: string) {
  return rimraf(path.join(store, pkgId))
}
