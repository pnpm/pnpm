var Promise = require('./promise')
var debug = require('debug')('unpm:install')
var config = require('./config')
var join = require('path').join
var mkdirp = require('./mkdirp')
var fetch = require('./fetch')
var resolve = require('./resolve')

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

  return resolve(pkg)
    .then(function (res) {
      var name = '' + res.name + '@' + res.version
      return mkdirp(join(ctx.modules, name))
        .then(function (_) { return fetch(_, res.dist.tarball, res.dist.shasum) })
    })
}
