import install, {InstalledPackage, InstallationOptions} from './install'
import {InstallContext} from './api/install'
import {Dependencies} from './types'

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
export default function installMultiple (ctx: InstallContext, requiredPkgsMap: Dependencies, optionalPkgsMap: Dependencies, modules: string, options: MultipleInstallationOptions): Promise<InstalledPackage[]> {
  requiredPkgsMap = requiredPkgsMap || {}
  optionalPkgsMap = optionalPkgsMap || {}

  const optionalPkgs = Object.keys(optionalPkgsMap)
    .map(pkgName => pkgMeta(pkgName, optionalPkgsMap[pkgName], true))

  const requiredPkgs = Object.keys(requiredPkgsMap)
    .filter(pkgName => !optionalPkgsMap[pkgName])
    .map(pkgName => pkgMeta(pkgName, requiredPkgsMap[pkgName], false))

  ctx.store.packages = ctx.store.packages || {}

  return Promise.all(optionalPkgs.concat(requiredPkgs).map(async function (pkg) {
    try {
      const dependency = await install(ctx, pkg, modules, options)

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
}

function pkgMeta (name: string, version: string, optional: boolean) {
  return {
    rawSpec: version === '*' ? name : `${name}@${version}`,
    optional
  }
}
