'use strict'
const resolve = require('path').resolve
const spawn = require('cross-spawn')
const pkgFullName = require('../pkg_full_name')
const getTarballName = require('./get_tarball_name')

/**
 * Resolves a package hosted on the local filesystem
 */

module.exports = function resolveLocal (pkg) {
  const dependencyPath = resolve(pkg.root, pkg.spec)

  if (dependencyPath.slice(-4) === '.tgz' || dependencyPath.slice(-7) === '.tar.gz') {
    const name = getTarballName(dependencyPath)
    return Promise.resolve({
      name,
      fullname: pkgFullName({
        name,
        version: [
          'file',
          removeLeadingSlash(dependencyPath)
        ].join(pkgFullName.delimiter)
      }),
      root: dependencyPath,
      dist: {
        remove: false,
        local: true,
        tarball: dependencyPath
      }
    })
  }

  return resolveFolder(dependencyPath)
}

function resolveFolder (dependencyPath) {
  return new Promise((resolve, reject) => {
    const proc = spawn('npm', ['pack'], {
      cwd: dependencyPath
    })

    let stdout = ''

    proc.stdout.on('data', data => {
      stdout += data.toString()
    })

    proc.on('error', reject)

    proc.on('close', code => {
      if (code > 0) return reject(new Error('Exit code ' + code))
      const tgzFilename = stdout.trim()
      return resolve(tgzFilename)
    })
  })
  .then(tgzFilename => {
    const localPkg = require(resolve(dependencyPath, 'package.json'))
    return {
      name: localPkg.name,
      fullname: pkgFullName({
        name: localPkg.name,
        version: [
          'file',
          removeLeadingSlash(dependencyPath)
        ].join(pkgFullName.delimiter)
      }),
      root: dependencyPath,
      dist: {
        remove: true,
        local: true,
        tarball: resolve(dependencyPath, tgzFilename)
      }
    }
  })
}

function removeLeadingSlash (pkgPath) {
  return pkgPath.replace(/^[/\\]/, '')
}
