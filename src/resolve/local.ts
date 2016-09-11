import {resolve} from 'path'
import spawn = require('cross-spawn')
import pkgFullName, {delimiter} from '../pkg_full_name'
import getTarballName from './get_tarball_name'
import requireJson from '../fs/require_json'
import {PackageToResolve} from '../resolve'

/**
 * Resolves a package hosted on the local filesystem
 */

export default async function resolveLocal (pkg: PackageToResolve) {
  const dependencyPath = resolve(pkg.root, pkg.spec)

  if (dependencyPath.slice(-4) === '.tgz' || dependencyPath.slice(-7) === '.tar.gz') {
    const name = getTarballName(dependencyPath)
    return {
      name,
      fullname: pkgFullName({
        name,
        version: [
          'file',
          removeLeadingSlash(dependencyPath)
        ].join(delimiter)
      }),
      root: dependencyPath,
      dist: {
        remove: false,
        local: true,
        tarball: dependencyPath
      }
    }
  }

  return resolveFolder(dependencyPath)
}

function resolveFolder (dependencyPath: string) {
  return new Promise((resolve, reject) => {
    const proc = spawn('npm', ['pack'], {
      cwd: dependencyPath
    })

    let stdout = ''

    proc.stdout.on('data', (data: Object) => {
      stdout += data.toString()
    })

    proc.on('error', reject)

    proc.on('close', (code: number) => {
      if (code > 0) return reject(new Error('Exit code ' + code))
      const tgzFilename = stdout.trim()
      return resolve(tgzFilename)
    })
  })
  .then(tgzFilename => {
    const localPkg = requireJson(resolve(dependencyPath, 'package.json'))
    return {
      name: localPkg.name,
      version: localPkg.version,
      fullname: pkgFullName({
        name: localPkg.name,
        version: [
          'file',
          removeLeadingSlash(dependencyPath)
        ].join(delimiter)
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

function removeLeadingSlash (pkgPath: string) {
  return pkgPath.replace(/^[/\\]/, '')
}
