import logger, {LoggedPkg} from 'pnpm-logger'
import fs = require('mz/fs')
import path = require('path')
import spawn = require('cross-spawn')
import execa = require('execa')
import {IncomingMessage} from 'http'
import * as unpackStream from 'unpack-stream'
import {Resolution} from '../resolve'
import {Got} from '../network/got'
import logStatus from '../logging/logInstallStatus'

const gitLogger = logger('git')

const fetchLogger = logger('fetch')

export type FetchOptions = {
  loggedPkg: LoggedPkg,
  got: Got
}

export type PackageDist = {
  tarball: string,
  shasum?: string
}

export default async function fetchResolution (
  resolution: Resolution,
  target: string,
  opts: FetchOptions
): Promise<void> {
  switch (resolution.type) {

    case 'tarball':
      const dist = {
        tarball: resolution.tarball,
        shasum: resolution.shasum,
      }
      await fetchFromTarball(target, dist, opts)
      break;

    case 'git-repo':
      await clone(resolution.repo, resolution.commitId, target)
      break;

    case 'directory': {
      const tgzFilename = await npmPack(resolution.root)
      const tarball = path.resolve(resolution.root, tgzFilename)
      const dist = {tarball: tarball}
      await fetchFromLocalTarball(target, dist)
      await fs.unlink(dist.tarball)
      break;
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
  const stream: IncomingMessage = await opts.got.getStream(dist.tarball)
  await unpackStream.remote(stream, dir, {
    shasum: dist.shasum,
    onStart: () => logStatus({status: 'fetching', pkg: opts.loggedPkg}),
    onProgress: (done: number, total: number) => logStatus({status: 'fetching', pkg: opts.loggedPkg, progress: { done, total }})
  })
  fetchLogger.debug(`finish ${dist.shasum} ${dist.tarball}`)
}

export async function fetchFromLocalTarball (dir: string, dist: PackageDist) {
  await unpackStream.local(fs.createReadStream(dist.tarball), dir)
}
