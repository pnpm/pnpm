import {resolve} from 'path'
import * as path from 'path'
import spawn = require('cross-spawn')
import getTarballName from './getTarballName'
import requireJson from '../fs/requireJson'
import mkdirp from '../fs/mkdirp'
import {PackageSpec, ResolveOptions, ResolveResult} from '.'
import {createLocalTarballFetcher, fetchFromLocalTarball} from './fetch'
import fs = require('mz/fs')
import linkDir from 'link-dir'

/**
 * Resolves a package hosted on the local filesystem
 */
export default async function resolveLocal (spec: PackageSpec, opts: ResolveOptions): Promise<ResolveResult> {
  const dependencyPath = resolve(opts.root, spec.spec)

  if (dependencyPath.slice(-4) === '.tgz' || dependencyPath.slice(-7) === '.tar.gz') {
    const name = getTarballName(dependencyPath)
    return {
      id: createLocalPkgId(name, dependencyPath),
      root: path.dirname(dependencyPath),
      fetch: createLocalTarballFetcher({
        tarball: dependencyPath
      })
    }
  }

  if (opts.linkLocal) {
    const localPkg = await requireJson(resolve(dependencyPath, 'package.json'))
    return {
      id: createLocalPkgId(localPkg.name, dependencyPath),
      root: dependencyPath,
      fetch: async function (target: string) {
        await mkdirp(path.dirname(target))
        return linkDir(dependencyPath, target)
      }
    }
  }
  return resolveFolder(dependencyPath)
}

async function resolveFolder (dependencyPath: string): Promise<ResolveResult> {
  const tgzFilename = await new Promise((resolve, reject) => {
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
  const localPkg = await requireJson(resolve(dependencyPath, 'package.json'))
  const dist = {
    tarball: resolve(dependencyPath, tgzFilename)
  }
  return {
    id: createLocalPkgId(localPkg.name, dependencyPath),
    root: dependencyPath,
    fetch: async function (target: string) {
      await fetchFromLocalTarball(target, dist)
      return fs.unlink(dist.tarball)
    }
  }
}

function createLocalPkgId (name: string, dependencyPath: string): string {
  return 'local/' + encodeURIComponent(dependencyPath)
}
