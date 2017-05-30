import logger from 'pnpm-logger'
import fs = require('mz/fs')
import path = require('path')
import spawn = require('cross-spawn')
import execa = require('execa')
import {IncomingMessage} from 'http'
import * as unpackStream from 'unpack-stream'
import existsFile = require('path-exists')
import {Resolution} from '../resolve'
import {Got} from '../network/got'
import logStatus from '../logging/logInstallStatus'
import parseNpmTarballUrl from 'parse-npm-tarball-url'
import parseCodeloadUrl from 'parse-codeload-url'
import {escapeHost} from '../resolve/npm/getRegistryName'
import {PnpmError} from '../errorTypes'

const gitLogger = logger('git')

const fetchLogger = logger('fetch')

export type FetchOptions = {
  pkgId: string,
  got: Got,
  localRegistry: string,
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
): Promise<void> {
  switch (resolution.type) {

    case undefined:
      const dist = {
        tarball: resolution.tarball,
        integrity: resolution.integrity,
        registry: resolution.registry,
      }
      await fetchFromTarball(target, dist, opts)
      break;

    case 'git':
      await clone(resolution.repo, resolution.commit, target)
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
  const localTarballPath = getLocalTarballPath(dist.tarball, opts.localRegistry)
  if (!await existsFile(localTarballPath)) {
    if (opts.offline) {
      throw new PnpmError('NO_OFFLINE_TARBALL', `Could not find ${localTarballPath} in local registry mirror ${opts.localRegistry}`)
    }
    await opts.got.download(dist.tarball, localTarballPath, {
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
  await fetchFromLocalTarball(dir, {
    integrity: dist.integrity,
    tarball: localTarballPath,
  })
  fetchLogger.debug(`finish ${dist.integrity} ${dist.tarball}`)
}

function getLocalTarballPath (tarballUrl: string, localRegistry: string) {
  const tarball = parseNpmTarballUrl(tarballUrl)
  if (tarball) {
    const escapedHost = escapeHost(tarball.host)
    return path.join(localRegistry, escapedHost, tarball.pkg.name,
      `${unscope(tarball.pkg.name)}-${tarball.pkg.version}.tgz`)
  }
  if (tarballUrl.includes('//codeload.github.com')) {
    const repo = parseCodeloadUrl(tarballUrl)
    return path.join(localRegistry, 'codeload.github.com', repo.owner,
      repo.name, `${repo.name}-${repo.commit}.tgz`)
  }
  return path.join(localRegistry,
    normalizeTarballExtension(tarballUrl.replace(/^.*:\/\/(git@)?/, '')))
}

function normalizeTarballExtension (tarballUrl: string) {
  if (tarballUrl.endsWith('.tar.gz')) {
    return `${tarballUrl.slice(0, -7)}.tgz`
  }
  if (tarballUrl.endsWith('.tgz')) {
    return tarballUrl
  }
  return `${tarballUrl}.tgz`
}

function unscope (pkgName: string) {
  if (pkgName[0] === '@') {
    return pkgName.split('/')[1]
  }
  return pkgName
}

async function fetchFromLocalTarball (dir: string, dist: PackageDist) {
  await unpackStream.local(fs.createReadStream(dist.tarball), dir)
}
