import mkdirp = require('mkdirp')
import createDebug from '../debug'
const debug = createDebug('pnpm:mkdirp')

/*
 * mkdir -p as a promise.
 */

export default function (path) {
  return new Promise((resolve, reject) => {
    debug(path)
    mkdirp(path, err => err ? reject(err) : resolve(path))
  })
}
