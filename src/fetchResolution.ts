import logger from '@pnpm/logger'
import dint = require('dint')
import execa = require('execa')
import {IncomingMessage} from 'http'
import fs = require('mz/fs')
import path = require('path')
import rimraf = require('rimraf-then')
import * as unpackStream from 'unpack-stream'
import {PnpmError} from './errorTypes'
import {progressLogger} from './loggers'
import {Got} from './network/got'
import {Resolution} from './resolve'

const gitLogger = logger('git')

const fetchLogger = logger('fetch')

export type IgnoreFunction = (filename: string) => boolean

export interface FetchOptions {
  pkgId: string,
  got: Got,
  storePath: string,
  offline: boolean,
  prefix: string,
  ignore?: IgnoreFunction,
}

export interface PackageDist {
  tarball: string,
  registry?: string,
  integrity?: string,
}

export default async function fetchResolution (
  resolution: Resolution,
  target: string,
  opts: FetchOptions,
): Promise<unpackStream.Index> {
  switch (resolution.type) {

    case undefined:
      const dist = {
        integrity: resolution.integrity,
        registry: resolution.registry,
        tarball: resolution.tarball,
      }
      return await fetchFromTarball(target, dist, opts) as unpackStream.Index

    case 'git':
      return await clone(resolution.repo, resolution.commit, target)

    default: {
      throw new Error(`Fetching for dependency type "${resolution.type}" is not supported`)
    }
  }
}

/**
 * clone a git repository.
 */
async function clone (repo: string, commitId: string, dest: string) {
  await execGit(['clone', repo, dest])
  await execGit(['checkout', commitId], {cwd: dest})
  // removing /.git to make directory integrity calculation faster
  await rimraf(path.join(dest, '.git'))
  const dirIntegrity = await dint.from(dest)
  return {
    headers: dirIntegrity,
    integrityPromise: Promise.resolve(dirIntegrity),
  }
}

function prefixGitArgs (): string[] {
  return process.platform === 'win32' ? ['-c', 'core.longpaths=true'] : []
}

function execGit (args: string[], opts?: object) {
  gitLogger.debug(`executing git with args ${args}`)
  const fullArgs = prefixGitArgs().concat(args || [])
  return execa('git', fullArgs, opts)
}

export function fetchFromTarball (dir: string, dist: PackageDist, opts: FetchOptions) {
  if (dist.tarball.startsWith('file:')) {
    dist = Object.assign({}, dist, {tarball: path.join(opts.prefix, dist.tarball.slice(5))})
    return fetchFromLocalTarball(dir, dist, opts.ignore)
  } else {
    return fetchFromRemoteTarball(dir, dist, opts)
  }
}

export async function fetchFromRemoteTarball (dir: string, dist: PackageDist, opts: FetchOptions) {
  const localTarballPath = path.join(opts.storePath, opts.pkgId, 'packed.tgz')
  try {
    const index = await fetchFromLocalTarball(dir, {
      integrity: dist.integrity,
      tarball: localTarballPath,
    }, opts.ignore)
    fetchLogger.debug(`finish ${dist.integrity} ${dist.tarball}`)
    return index
  } catch (err) {
    if (err.code !== 'ENOENT') throw err

    if (opts.offline) {
      throw new PnpmError('NO_OFFLINE_TARBALL', `Could not find ${localTarballPath} in local registry mirror ${opts.storePath}`)
    }
    return await opts.got.download(dist.tarball, localTarballPath, {
      ignore: opts.ignore,
      integrity: dist.integrity,
      onProgress: (downloaded) => {
        progressLogger.debug({status: 'fetching_progress', pkgId: opts.pkgId, downloaded})
      },
      onStart: (size, attempt) => {
        progressLogger.debug({status: 'fetching_started', pkgId: opts.pkgId, size, attempt})
      },
      registry: dist.registry,
      unpackTo: dir,
    })
  }
}

async function fetchFromLocalTarball (
  dir: string,
  dist: PackageDist,
  ignore?: IgnoreFunction,
): Promise<unpackStream.Index> {
  return await unpackStream.local(
    fs.createReadStream(dist.tarball),
    dir,
    {
      ignore,
    },
  ) as unpackStream.Index
}
