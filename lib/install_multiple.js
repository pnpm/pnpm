'use strict'
const install = require('./install')
const pkgFullName = require('./pkg_full_name')

/*
 * Install multiple modules into `modules`.
 *
 *     ctx = { }
 *     installMultiple(ctx, { minimatch: '^2.0.0' }, {chokidar: '^1.6.0'}, './node_modules')
 */

module.exports = function installMultiple (ctx, requiredPkgsMap, optionalPkgsMap, modules, options) {
  requiredPkgsMap = requiredPkgsMap || {}
  optionalPkgsMap = optionalPkgsMap || {}

  const optionalPkgs = Object.keys(optionalPkgsMap)
    .map(pkgName => pkgMeta(pkgName, optionalPkgsMap[pkgName], true))

  const requiredPkgs = Object.keys(requiredPkgsMap)
    .filter(pkgName => !optionalPkgsMap[pkgName])
    .map(pkgName => pkgMeta(pkgName, requiredPkgsMap[pkgName], false))

  ctx.dependents = ctx.dependents || {}
  ctx.dependencies = ctx.dependencies || {}

  return Promise.all(optionalPkgs.concat(requiredPkgs).map(pkg => install(ctx, pkg, modules, options)
    .then(dependency => {
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
    })
    .catch(err => {
      if (pkg.optional) {
        console.log('Skipping failed optional dependency ' + pkg.rawSpec + ':')
        console.log(err.message || err)
        return
      }
      throw err
    })))
}

function pkgMeta (name, version, optional) {
  return {
    rawSpec: version ? '' + name + '@' + version : name,
    optional
  }
}
