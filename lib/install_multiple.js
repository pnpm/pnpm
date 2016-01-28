var install = require('./install')
var debug = require('debug')('unpm:install_all')

/*
 * Install multiple modules.
 *
 *     ctx = { modules: './node_modules' }
 *     installMultiple(ctx, ['minimatch@2'])
 *     installMultiple(ctx, { minimatch: '^2.0.0' })
 */

module.exports = function installMultiple (ctx, pkgs, options) {
  pkgs = arrayify(pkgs)
  return Promise.all(pkgs.map(function (pkg) {
    return install(ctx, pkg, options)
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
