var Promise = require('./promise')
var debug = require('debug')('unpm:install')
var config = require('./config')
var join = require('path').join
var mkdirp = require('./mkdirp')
var fetch = require('./fetch')
var resolve = require('./resolve')
var getUuid = require('node-uuid')
var symlink = require('./force_symlink')
var fs = require('mz/fs')

/*
 * Installs a package.
 * Parameters:
 *
 * - `ctx` (Object) - the context.
 *   - `modules` (String) - node modules path.
 *
 * What it does:
 *
 * - resolve() - resolve from registry.npmjs.org
 * - fetch() - download tarball into node_modules/.tmp/{uuid}
 * - recurse into its dependencies
 * - run postinstall hooks
 * - move .tmp/{uuid} into node_modules/{name}@{version}
 * - symlink node_modules/{name}
 * - symlink bins
 */

module.exports = function install (ctx, pkg, options) {
  debug('installing ' + pkg)

  var uuid = getUuid()
  var name, fullname, dist

  // TODO: check existence of .store/minimatch@3.0.0
  return resolve(pkg)
    .then(set)
    .then(_ => mkdirp(join(ctx.store)))
    .then(_ => mkdirp(join(ctx.tmp, uuid)))
    .then(dir => fetch(dir, dist.tarball, dist.shasum))
    .then(_ => fs.rename(join(ctx.tmp, uuid), join(ctx.store, fullname)))
    .then(_ => symlink(join(ctx.store, fullname), join(ctx.modules, name)))

  function set (res) {
    fullname = '' + res.name + '@' + res.version
    name = res.name
    dist = res.dist
  }
}
