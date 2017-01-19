import logger, {LoggedPkg} from 'pnpm-logger'
import fs = require('mz/fs')
import linkDir from 'link-dir'
import path = require('path')
import spawn = require('cross-spawn')
import execa = require('execa')
import {IncomingMessage} from 'http'
import * as unpackStream from 'unpack-stream'

import {Resolution} from '../resolve'
import mkdirp from '../fs/mkdirp'
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
    case 'package':
      const dist = {
        tarball: resolution.tarball,
        shasum: resolution.shasum,
      }
      await fetchFromRemoteTarball(target, dist, opts)
      break;

    case 'git-repo':
      if (resolution.tarball) {
        const dist = {tarball: resolution.tarball}
        await fetchFromRemoteTarball(target, dist, opts)
      } else {
        await clone(resolution.repo, resolution.commitId, target)
      }
      break;

    case 'local-tarball': {
      const dist = {tarball: resolution.tarball}
      await fetchFromLocalTarball(target, dist)
      break;
    }

    case 'directory': {
      const tgzFilename = await npmPack(resolution.root)
      const tarball = path.resolve(resolution.root, tgzFilename)
      const dist = {tarball: tarball}
      await fetchFromLocalTarball(target, dist)
      await fs.unlink(dist.tarball)
      break;
    }

    case 'link':
      await mkdirp(path.dirname(target))
      await linkDir(resolution.root, target)
      break;
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

/**
 * Fetches a tarball `tarball` and extracts it into `dir`
 */
export async function fetchFromRemoteTarball (dir: string, dist: PackageDist, opts: FetchOptions) {
  const stream: IncomingMessage = await opts.got.getStream(dist.tarball)
  await unpackStream.remote(stream, dir, {
    shasum: dist.shasum,
    onStart: () => logStatus({status: 'download-start', pkg: opts.loggedPkg}),
    onProgress: (done: number, total: number) => logStatus({status: 'downloading', pkg: opts.loggedPkg, downloadStatus: { done, total }})
  })
  fetchLogger.debug(`finish ${dist.shasum} ${dist.tarball}`)
}

export async function fetchFromLocalTarball (dir: string, dist: PackageDist) {
  await unpackStream.local(fs.createReadStream(dist.tarball), dir)
}
