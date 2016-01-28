var Promise = require('./promise')
var debug = require('debug')('unpm:install')
var npa = require('npm-package-arg')
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
 *
 *     install(ctx, 'rimraf@2')
 *
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
  var installAll = require('./install_multiple')
  debug('installing ' + pkg)

  var depth = (options && options.depth || 0)
  var pkgSpec = npa(pkg)
  var pkgData, name, fullname, dist, target

  return fs.stat(join(ctx.modules, pkgSpec.name))
    .catch((err) => {
      return resolve(pkgSpec)
        .then(set)
        .then(_ => fs.stat(target))
        .catch((err) => {
          if (isLocked(target)) return Promise.resolve()
          return Promise.resolve()
          .then(_ => lock(target))
          .then(_ => mkdirp(join(ctx.modules, '.store')))
          .then(dir => fetchIf(ctx.tmp, target, dist.tarball, dist.shasum))
          .then(_ => recurseDependencies())
          .then(_ => symlink(join('.store', fullname), join(ctx.modules, name)))
          .then(_ => unlock(target))
        })
    })

  function set (res) {
    pkgData = res
    fullname = '' + res.name + '@' + res.version
    target = join(ctx.modules, '.store', fullname)
    name = res.name
    dist = res.dist
  }

  function lock (path) {
    if (!ctx.lock) ctx.lock = {}
    ctx.lock[path] = true
  }

  function unlock (path) {
    if (ctx.lock) ctx.lock[path] = undefined
  }

  function isLocked (path) {
    return ctx.lock && ctx.lock[path]
  }

  function recurseDependencies () {
    // TODO: install to proper node_modules
    return installAll(ctx, pkgData.dependencies, { depth: depth + 1 })
  }
}

/*
 * Idempotent version of fetch()
 */

function fetchIf (tmpDir, target, tarball, shasum) {
  var uuid = getUuid()
  var tmp = join(tmpDir, uuid)

  return fs.stat(target)
    .then(_ => target)
    .catch(() => {
      return Promise.resolve()
        .then(_ => mkdirp(tmp))
        .then(_ => fetch(tmp, tarball, shasum))
        .then(_ => fs.rename(tmp, target))
        .then(_ => target)
    })
}
