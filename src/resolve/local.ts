import {resolve} from 'path'
import spawn = require('cross-spawn')
import pkgFullName, {delimiter} from '../pkgFullName'
import getTarballName from './getTarballName'
import requireJson from '../fs/requireJson'
import {PackageSpec} from '../install'
import {ResolveOptions, ResolveResult} from '.'

/**
 * Resolves a package hosted on the local filesystem
 */
export default async function resolveLocal (spec: PackageSpec, opts: ResolveOptions): Promise<ResolveResult> {
  const dependencyPath = resolve(opts.root, spec.spec)

  if (dependencyPath.slice(-4) === '.tgz' || dependencyPath.slice(-7) === '.tar.gz') {
    const name = getTarballName(dependencyPath)
    return {
      name,
      fullname: getFullName(name, dependencyPath),
      dist: {
        location: 'local',
        tarball: dependencyPath
      }
    }
  }

  if (opts.linkLocal) {
    const localPkg = requireJson(resolve(dependencyPath, 'package.json'))
    return {
      fullname: getFullName(localPkg.name, dependencyPath),
      dist: {
        location: 'dir',
        tarball: dependencyPath,
      }
    }
  }
  return resolveFolder(dependencyPath)
}

function resolveFolder (dependencyPath: string): Promise<ResolveResult> {
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
      fullname: getFullName(localPkg.name, dependencyPath),
      dist: {
        location: 'dir',
        tarball: resolve(dependencyPath, tgzFilename)
      }
    }
  })
}

function getFullName (name: string, dependencyPath: string): string {
  return pkgFullName({
    name,
    version: [
      'file',
      removeLeadingSlash(dependencyPath)
    ].join(delimiter)
  })
}

function removeLeadingSlash (pkgPath: string): string {
  return pkgPath.replace(/^[/\\]/, '')
}
