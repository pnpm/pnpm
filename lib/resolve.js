var resolveNpm = require('./resolve/npm')

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

module.exports = function resolve (pkg) {
  if (pkg.type === 'range' || pkg.type === 'version' || pkg.type === 'tag') {
    return resolveNpm(pkg)
  } else {
    throw new Error('' + pkg.rawSpec + ': ' + pkg.type + ' packages not supported')
  }
}
