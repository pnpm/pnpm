'use strict'
const debug = require('debug')('pnpm:fetch')
const crypto = require('crypto')
const gunzip = require('gunzip-maybe')
const tar = require('tar-fs')
const fs = require('fs')

/*
 * Fetches a tarball `tarball` and extracts it into `dir`
 */

module.exports = function fetch (dir, dist, opts) {
  if (!dist.local) {
    return opts.got.getStream(dist.tarball)
      .then(stream => fetchStream(dir, dist.tarball, dist.shasum, opts.log, stream))
  }
  return unpackStream(fs.createReadStream(dist.tarball), dir)
}

function fetchStream (dir, tarball, shasum, log, stream) {
  return new Promise((resolve, reject) => {
    const actualShasum = crypto.createHash('sha1')
    let size
    let downloaded = 0

    unpackStream(
      stream
        .on('response', start)
        .on('data', _ => { actualShasum.update(_) })
        .on('error', reject), dir
    ).then(finish)

    function start (res) {
      if (res.statusCode !== 200) {
        return reject(new Error('' + tarball + ': invalid response ' + res.statusCode))
      }

      log('download-start')
      if ('content-length' in res.headers) {
        size = +res.headers['content-length']
        res.on('data', chunk => {
          downloaded += chunk.length
          log('downloading', { done: downloaded, total: size })
        })
      }
    }

    function finish () {
      const digest = actualShasum.digest('hex')
      debug(`finish ${shasum} ${tarball}`)
      if (shasum && digest !== shasum) {
        return reject(new Error('' + tarball + ': incorrect shasum (expected ' + shasum + ', got ' + digest + ')'))
      }

      return resolve(dir)
    }
  })
}

function unpackStream (stream, dir) {
  return new Promise((resolve, reject) => {
    stream
      .pipe(gunzip()).on('error', reject)
      .pipe(tar.extract(dir, { strip: 1 })).on('error', reject)
      .on('finish', resolve)
  })
}
