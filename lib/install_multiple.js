var install = require('./install')

/*
 * Install multiple modules into `modules`.
 *
 *     ctx = { }
 *     installMultiple(ctx, ['minimatch@2'], './node_modules')
 *     installMultiple(ctx, { minimatch: '^2.0.0' }, './node_modules')
 */

module.exports = function installMultiple (ctx, pkgs, modules, options) {
  pkgs = arrayify(pkgs)
  return Promise.all(pkgs.map(function (pkg) {
    return install(ctx, pkg, modules, options)
  }))
}

function arrayify (pkgs) {
  if (!pkgs) return []
  if (typeof pkgs !== 'object') return [ pkgs ]
  if (Array.isArray(pkgs)) return pkgs

  return Object.keys(pkgs).map((name) => {
    return '' + name + '@' + pkgs[name]
  })
}
