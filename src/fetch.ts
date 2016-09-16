import createDebug from './debug'
const debug = createDebug('pnpm:fetch')
import crypto = require('crypto')
import gunzip = require('gunzip-maybe')
import tar = require('tar-fs')
import fs = require('fs')
import {PackageDist} from './resolve'
import {Got} from './network/got'
import {InstallLog} from './install'
import {IncomingMessage} from 'http'

export type FetchOptions = {
  log: InstallLog,
  got: Got
}

/*
 * Fetches a tarball `tarball` and extracts it into `dir`
 */

export default function fetch (dir: string, dist: PackageDist, opts: FetchOptions) {
  if (dist.location === 'remote') {
    return opts.got.getStream(dist.tarball)
      .then((stream: NodeJS.ReadableStream) => fetchStream(dir, dist, opts.log, stream))
  }
  return unpackStream(fs.createReadStream(dist.tarball), dir)
}

function fetchStream (dir: string, dist: PackageDist, log: InstallLog, stream: NodeJS.ReadableStream) {
  return new Promise((resolve, reject) => {
    const actualShasum = crypto.createHash('sha1')
    let size = 0
    let downloaded = 0

    unpackStream(
      stream
        .on('response', start)
        .on('data', (_: Buffer) => { actualShasum.update(_) })
        .on('error', reject), dir
    ).then(finish)

    function start (res: IncomingMessage) {
      if (res.statusCode !== 200) {
        return reject(new Error('' + dist.tarball + ': invalid response ' + res.statusCode))
      }

      log('download-start')
      if ('content-length' in res.headers) {
        size = +res.headers['content-length']
        res.on('data', (chunk: Buffer) => {
          downloaded += chunk.length
          log('downloading', { done: downloaded, total: size })
        })
      }
    }

    function finish () {
      const digest = actualShasum.digest('hex')
      debug(`finish ${dist.shasum} ${dist.tarball}`)
      if (dist.shasum && digest !== dist.shasum) {
        return reject(new Error('' + dist.tarball + ': incorrect shasum (expected ' + dist.shasum + ', got ' + digest + ')'))
      }

      return resolve(dir)
    }
  })
}

function unpackStream (stream: NodeJS.ReadableStream, dir: string) {
  return new Promise((resolve, reject) => {
    stream
      .pipe(gunzip()).on('error', reject)
      .pipe(tar.extract(dir, { strip: 1 })).on('error', reject)
      .on('finish', resolve)
  })
}
