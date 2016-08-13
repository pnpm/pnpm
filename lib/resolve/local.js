var resolve = require('path').resolve
var spawn = require('cross-spawn')

/**
 * Resolves a package hosted on the local filesystem
 */

module.exports = function resolveLocal (pkg) {
  var dependencyPath = resolve(pkg.root, pkg.spec)

  return new Promise((resolve, reject) => {
    var proc = spawn('npm', ['pack'], {
      cwd: dependencyPath
    })

    proc.on('error', reject)

    proc.on('close', code => {
      if (code > 0) return reject(new Error('Exit code ' + code))
      return resolve()
    })
  })
  .then(_ => {
    var localPkg = require(resolve(dependencyPath, 'package.json'))
    var tgzFilename = localPkg.name + '-' + localPkg.version + '.tgz'
    return {
      name: localPkg.name,
      fullname: localPkg.name.replace('/', '!') + [
        '@file',
        escapePkgPath(dependencyPath)
      ].join('!'),
      dist: {
        local: true,
        tarball: resolve(dependencyPath, tgzFilename)
      }
    }
  })
}

function escapePkgPath (pkgPath) {
  return pkgPath.replace(/[/\\:]/g, '!').replace(/^!/, '')
}
