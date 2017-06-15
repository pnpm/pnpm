import logger from 'pnpm-logger'
import fs = require('mz/fs')
import path = require('path')
import spawn = require('cross-spawn')
import execa = require('execa')
import {IncomingMessage} from 'http'
import * as unpackStream from 'unpack-stream'
import existsFile = require('path-exists')
import dint = require('dint')
import {Resolution} from '../resolve'
import {Got} from '../network/got'
import logStatus from '../logging/logInstallStatus'
import {escapeHost} from '../resolve/npm/getRegistryName'
import {PnpmError} from '../errorTypes'
import rimraf = require('rimraf-then')

const gitLogger = logger('git')

const fetchLogger = logger('fetch')

export type FetchOptions = {
  pkgId: string,
  got: Got,
  storePath: string,
  offline: boolean,
}

export type PackageDist = {
  tarball: string,
  registry?: string,
  integrity?: string,
}

export default async function fetchResolution (
  resolution: Resolution,
  target: string,
  opts: FetchOptions
): Promise<unpackStream.Index> {
  switch (resolution.type) {

    case undefined:
      const dist = {
        tarball: resolution.tarball,
        integrity: resolution.integrity,
        registry: resolution.registry,
      }
      return await fetchFromTarball(target, dist, opts) as unpackStream.Index

    case 'git':
      return await clone(resolution.repo, resolution.commit, target)

    case 'directory': {
      const tgzFilename = await npmPack(resolution.directory)
      const tarball = path.resolve(resolution.directory, tgzFilename)
      const dist = {tarball: tarball}
      const index = await fetchFromLocalTarball(target, dist)
      await fs.unlink(dist.tarball)
      return index
    }
  }
}

function npmPack(dependencyPath: string): Promise<string> {
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
}

/**
 * clone a git repository.
 */
async function clone (repo: string, commitId: string, dest: string) {
  await execGit(['clone', repo, dest])
  await execGit(['checkout', commitId], {cwd: dest})
  // removing /.git to make directory integrity calculation faster
  await rimraf(path.join(dest, '.git'))
  const dirIntegrity = dint.from(dest)
  return {
    headers: dirIntegrity,
    integrityPromise: Promise.resolve(dirIntegrity),
  }
}

function prefixGitArgs (): string[] {
  return process.platform === 'win32' ? ['-c', 'core.longpaths=true'] : []
}

function execGit (args: string[], opts?: Object) {
  gitLogger.debug(`executing git with args ${args}`)
  const fullArgs = prefixGitArgs().concat(args || [])
  return execa('git', fullArgs, opts)
}

export function fetchFromTarball (dir: string, dist: PackageDist, opts: FetchOptions) {
  if (dist.tarball.startsWith('file:')) {
    dist = Object.assign({}, dist, {tarball: dist.tarball.slice(5)})
    return fetchFromLocalTarball(dir, dist)
  } else {
    return fetchFromRemoteTarball(dir, dist, opts)
  }
}

export async function fetchFromRemoteTarball (dir: string, dist: PackageDist, opts: FetchOptions) {
  const localTarballPath = path.join(opts.storePath, opts.pkgId, 'packed.tgz')
  if (!await existsFile(localTarballPath)) {
    if (opts.offline) {
      throw new PnpmError('NO_OFFLINE_TARBALL', `Could not find ${localTarballPath} in local registry mirror ${opts.storePath}`)
    }
    return await opts.got.download(dist.tarball, localTarballPath, {
      unpackTo: dir,
      registry: dist.registry,
      integrity: dist.integrity,
      onStart: () => logStatus({status: 'fetching', pkgId: opts.pkgId}),
      onProgress: (done: number, total: number) =>
        logStatus({
          status: 'fetching',
          pkgId: opts.pkgId,
          progress: { done, total },
        })
    })
  }
  const index = await fetchFromLocalTarball(dir, {
    integrity: dist.integrity,
    tarball: localTarballPath,
  })
  fetchLogger.debug(`finish ${dist.integrity} ${dist.tarball}`)
  return index
}

async function fetchFromLocalTarball (
  dir: string,
  dist: PackageDist
): Promise<unpackStream.Index> {
  return await unpackStream.local(fs.createReadStream(dist.tarball), dir) as unpackStream.Index
}
