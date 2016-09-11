import install, {PackageContext, InstallationOptions} from './install'
import pkgFullName from './pkg_full_name'
import {InstallContext} from './api/install'

export type Dependencies = {
  [name: string]: string
}

export type MultipleInstallationOptions = InstallationOptions & {
  dependent: string
}

/*
 * Install multiple modules into `modules`.
 *
 *     ctx = { }
 *     installMultiple(ctx, { minimatch: '^2.0.0' }, {chokidar: '^1.6.0'}, './node_modules')
 */

export default function installMultiple (ctx: InstallContext, requiredPkgsMap: Dependencies, optionalPkgsMap: Dependencies, modules: string, options: MultipleInstallationOptions): Promise<PackageContext[]> {
  requiredPkgsMap = requiredPkgsMap || {}
  optionalPkgsMap = optionalPkgsMap || {}

  const optionalPkgs = Object.keys(optionalPkgsMap)
    .map(pkgName => pkgMeta(pkgName, optionalPkgsMap[pkgName], true))

  const requiredPkgs = Object.keys(requiredPkgsMap)
    .filter(pkgName => !optionalPkgsMap[pkgName])
    .map(pkgName => pkgMeta(pkgName, requiredPkgsMap[pkgName], false))

  ctx.dependents = ctx.dependents || {}
  ctx.dependencies = ctx.dependencies || {}

  return Promise.all(optionalPkgs.concat(requiredPkgs).map(async function (pkg) {
    try {
      const dependency = await install(ctx, pkg, modules, options)
      const depFullName = pkgFullName(dependency)
      ctx.dependents[depFullName] = ctx.dependents[depFullName] || []
      if (ctx.dependents[depFullName].indexOf(options.dependent) === -1) {
        ctx.dependents[depFullName].push(options.dependent)
      }
      ctx.dependencies[options.dependent] = ctx.dependencies[options.dependent] || []
      if (ctx.dependencies[options.dependent].indexOf(depFullName) === -1) {
        ctx.dependencies[options.dependent].push(depFullName)
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
    rawSpec: version ? '' + name + '@' + version : name,
    optional
  }
}
