var debug = require('debug')('pnpm:fetch')
var got = require('./got')
var crypto = require('crypto')
var gunzip = require('gunzip-maybe')
var tar = require('tar-fs')

/*
 * Fetches a tarball `tarball` and extracts it into `dir`
 */

module.exports = function fetch (dir, tarball, shasum, log) {
  return new Promise((resolve, reject) => {
    var stream = got.stream(tarball)
    var actualShasum = crypto.createHash('sha1')
    var size
    var downloaded = 0

    stream
      .on('response', start)
      .on('data', (_) => { actualShasum.update(_) })
      .on('error', reject)
      .pipe(gunzip()).on('error', reject)
      .pipe(tar.extract(dir, { strip: 1 })).on('error', reject)
      .on('finish', finish)

    function start (res) {
      if (res.statusCode !== 200) {
        return reject(new Error('' + tarball + ': invalid response ' + res.statusCode))
      }

      if ('content-length' in res.headers) {
        size = +res.headers['content-length']
        res.on('data', (chunk) => {
          downloaded += chunk.length
          log('downloading', { done: downloaded, total: size })
        })
      }
    }

    function finish () {
      var digest = actualShasum.digest('hex')
      debug('finish %s', shasum)
      if (shasum && digest !== shasum) {
        return reject(new Error('' + tarball + ': incorrect shasum (expected ' + shasum + ', got ' + digest + ')'))
      }

      return resolve(dir)
    }
  })
}
