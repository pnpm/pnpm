import path = require('path')
import install, {InstalledPackage, InstallationOptions} from './install'
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
export default async function installMultiple (ctx: InstallContext, pkgsMap: Dependencies, modules: string, options: MultipleInstallationOptions): Promise<InstalledPackage[]> {
  pkgsMap = pkgsMap || {}

  const pkgs = Object.keys(pkgsMap).map(pkgName => getRawSpec(pkgName, pkgsMap[pkgName]))

  ctx.store.packages = ctx.store.packages || {}

  const installedPkgs: InstalledPackage[] = <InstalledPackage[]>(
    await Promise.all(pkgs.map(async function (pkgRawSpec: string) {
      try {
        const dependency = await install(ctx.fetches, pkgRawSpec, modules, options)

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
            const nextInstallOpts = Object.assign({}, options, {
                keypath: dependency.keypath.concat([ dependency.id ]),
                dependent: dependency.id,
                optional: dependency.optional,
                root: dependency.srcPath,
              })
            dependency.dependencies = (await installMultiple(ctx,
                dependency.pkg.dependencies || {},
                path.join(dependency.path, 'node_modules'),
                nextInstallOpts)
            ).concat(
              await installMultiple(ctx,
                dependency.pkg.optionalDependencies || {},
                path.join(dependency.path, 'node_modules'),
                Object.assign({}, nextInstallOpts, {
                  optional: true
                }))
            )
            ctx.piq = ctx.piq || []
            ctx.piq.push({
              path: dependency.path,
              pkgId: dependency.id
            })
          }
        }

        return dependency
      } catch (err) {
        if (options.optional) {
          console.log(`Skipping failed optional dependency ${pkgRawSpec}:`)
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

function getRawSpec (name: string, version: string) {
  return version === '*' ? name : `${name}@${version}`
}
