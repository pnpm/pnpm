import createDebug from '../debug'
const debug = createDebug('pnpm:fetch')
import fs = require('fs')
import {Got} from '../network/got'
import {InstallLog} from '../install/fetch'
import {IncomingMessage} from 'http'
import * as unpackStream from 'unpack-stream'

export type FetchOptions = {
  log: InstallLog,
  got: Got
}

export type PackageDist = {
  tarball: string,
  shasum?: string
}

export function createRemoteTarballFetcher (dist: PackageDist, opts: FetchOptions) {
  return (target: string) => fetchFromRemoteTarball(target, dist, opts)
}

export function createLocalTarballFetcher (dist: PackageDist) {
  return (target: string) => fetchFromLocalTarball(target, dist)
}

/**
 * Fetches a tarball `tarball` and extracts it into `dir`
 */
export async function fetchFromRemoteTarball (dir: string, dist: PackageDist, opts: FetchOptions) {
  const stream: IncomingMessage = await opts.got.getStream(dist.tarball)
  await unpackStream.remote(stream, dir, {
    shasum: dist.shasum,
    onStart: () => opts.log('download-start'),
    onProgress: (done: number, total: number) => opts.log('downloading', { done, total })
  })
  debug(`finish ${dist.shasum} ${dist.tarball}`)
}

export async function fetchFromLocalTarball (dir: string, dist: PackageDist) {
  await unpackStream.local(fs.createReadStream(dist.tarball), dir)
}
