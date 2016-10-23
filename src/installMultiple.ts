import path = require('path')
import install, {InstalledPackage, InstallationOptions, PackageMeta} from './install'
import {InstallContext, InstalledPackages} from './api/install'
import {Dependencies} from './types'
import linkBins from './install/linkBins'

export type MultipleInstallationOptions = InstallationOptions & {
  dependent: string
}

/**
 * Install multiple modules into `modules`.
 *
 * @example
 *     ctx = { }
 *     installMultiple(ctx, { minimatch: '^2.0.0' }, {chokidar: '^1.6.0'}, './node_modules')
 */
export default async function installMultiple (ctx: InstallContext, requiredPkgsMap: Dependencies, optionalPkgsMap: Dependencies, modules: string, options: MultipleInstallationOptions): Promise<InstalledPackage[]> {
  requiredPkgsMap = requiredPkgsMap || {}
  optionalPkgsMap = optionalPkgsMap || {}

  const optionalPkgs = Object.keys(optionalPkgsMap)
    .map(pkgName => pkgMeta(pkgName, optionalPkgsMap[pkgName], true))

  const requiredPkgs = Object.keys(requiredPkgsMap)
    .filter(pkgName => !optionalPkgsMap[pkgName])
    .map(pkgName => pkgMeta(pkgName, requiredPkgsMap[pkgName], false))

  ctx.store.packages = ctx.store.packages || {}

  const installedPkgs: InstalledPackage[] = <InstalledPackage[]>(
    await Promise.all(optionalPkgs.concat(requiredPkgs).map(async function (pkg: PackageMeta) {
      try {
        const dependency = await install(ctx, pkg, modules, options)

        addInstalledPkg(ctx.installs, dependency)

        ctx.store.packages[options.dependent] = ctx.store.packages[options.dependent] || {}
        ctx.store.packages[options.dependent].dependencies = ctx.store.packages[options.dependent].dependencies || {}

        // NOTE: the current install implementation
        // does not return enough info for packages that were already installed
        if (!dependency.fromCache) {
          ctx.store.packages[options.dependent].dependencies[dependency.pkg.name] = dependency.id

          ctx.store.packages[dependency.id] = ctx.store.packages[dependency.id] || {}
          ctx.store.packages[dependency.id].dependents = ctx.store.packages[dependency.id].dependents || []
          if (ctx.store.packages[dependency.id].dependents.indexOf(options.dependent) === -1) {
            ctx.store.packages[dependency.id].dependents.push(options.dependent)
          }

          if (dependency.justFetched || dependency.firstFetch && dependency.keypath.length <= options.depth) {
            dependency.dependencies = await installMultiple(ctx,
              dependency.pkg.dependencies || {},
              dependency.pkg.optionalDependencies || {},
              path.join(dependency.path, 'node_modules'),
              {
                keypath: dependency.keypath.concat([ dependency.id ]),
                dependent: dependency.id,
                parentRoot: dependency.srcPath,
                optional: dependency.optional,
                linkLocal: options.linkLocal,
                root: options.root,
                storePath: options.storePath,
                force: options.force,
                depth: options.depth,
                tag: options.tag,
              })
            ctx.piq = ctx.piq || []
            ctx.piq.push({
              path: dependency.path,
              pkgId: dependency.id
            })
          }
        }

        return dependency
      } catch (err) {
        if (pkg.optional) {
          console.log('Skipping failed optional dependency ' + pkg.rawSpec + ':')
          console.log(err.message || err)
          return null // is it OK to return null?  
        }
        throw err
      }
    }))
  )
  .filter(pkg => pkg)

  await linkBins(modules)

  return installedPkgs
}

function addInstalledPkg (installs: InstalledPackages, newPkg: InstalledPackage) {
  if (!installs[newPkg.id]) {
    installs[newPkg.id] = newPkg
    return
  }
  installs[newPkg.id].optional = installs[newPkg.id].optional && newPkg.optional
}

function pkgMeta (name: string, version: string, optional: boolean) {
  return {
    rawSpec: version === '*' ? name : `${name}@${version}`,
    optional
  }
}
