var resolveNpm = require('./resolve/npm')
var resolveTarball = require('./resolve/tarball')
var resolveGithub = require('./resolve/github')
var resolveLocal = require('./resolve/local')

/**
 * Resolves a package in the NPM registry. Done as part of `install()`.
 *
 *     var npa = require('npm-package-arg')
 *     resolve(npa('rimraf@2'))
 *       .then((res) => {
 *         res.fullname == 'rimraf@2.5.1'
 *         res.dist == {
 *           shasum: '0a1b2c...'
 *           tarball: 'http://...'
 *         }
 *       })
 */

module.exports = function resolve (pkg, log) {
  if (pkg.type === 'range' || pkg.type === 'version' || pkg.type === 'tag') {
    return resolveNpm(pkg, log)
  } else if (pkg.type === 'remote') {
    return resolveTarball(pkg, log)
  } else if (pkg.type === 'hosted' && pkg.hosted.type === 'github') {
    return resolveGithub(pkg, log)
  } else if (pkg.type === 'local') {
    return resolveLocal(pkg, log)
  } else {
    throw new Error('' + pkg.rawSpec + ': ' + pkg.type + ' packages not supported')
  }
}
