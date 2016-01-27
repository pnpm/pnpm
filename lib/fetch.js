var debug = require('debug')('cnpm:fetch')
var got = require('./got')

/*
 * Fetches something
 */

module.exports = function fetch (dir, tarball, shasum) {
  return got(tarball)
  /*
  stream.on('data', actualShasum.update.bind(actualShasum))
    .on('error', cb)
    .pipe(gunzip()).on('error', cb)
    .pipe(untar).on('error', cb)
    .on('finish', onFinish)
  */
}
