import install, {InstalledPackage, InstallationOptions} from './install'
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

export default function installMultiple (ctx: InstallContext, requiredPkgsMap: Dependencies, optionalPkgsMap: Dependencies, modules: string, options: MultipleInstallationOptions): Promise<InstalledPackage[]> {
  requiredPkgsMap = requiredPkgsMap || {}
  optionalPkgsMap = optionalPkgsMap || {}

  const optionalPkgs = Object.keys(optionalPkgsMap)
    .map(pkgName => pkgMeta(pkgName, optionalPkgsMap[pkgName], true))

  const requiredPkgs = Object.keys(requiredPkgsMap)
    .filter(pkgName => !optionalPkgsMap[pkgName])
    .map(pkgName => pkgMeta(pkgName, requiredPkgsMap[pkgName], false))

  ctx.storeJson.dependents = ctx.storeJson.dependents || {}
  ctx.storeJson.dependencies = ctx.storeJson.dependencies || {}

  return Promise.all(optionalPkgs.concat(requiredPkgs).map(async function (pkg) {
    try {
      const dependency = await install(ctx, pkg, modules, options)
      const depFullName = dependency.fullname
      ctx.storeJson.dependents[depFullName] = ctx.storeJson.dependents[depFullName] || []
      if (ctx.storeJson.dependents[depFullName].indexOf(options.dependent) === -1) {
        ctx.storeJson.dependents[depFullName].push(options.dependent)
      }
      ctx.storeJson.dependencies[options.dependent] = ctx.storeJson.dependencies[options.dependent] || []
      if (ctx.storeJson.dependencies[options.dependent].indexOf(depFullName) === -1) {
        ctx.storeJson.dependencies[options.dependent].push(depFullName)
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
