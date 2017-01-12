import mkdirp = require('mkdirp')
import bole = require('bole')

const logger = bole('pnpm:mkdirp')

/**
 * mkdir -p as a promise.
 */
export default function (path: string) {
  return new Promise((resolve, reject) => {
    logger.debug(path)
    mkdirp(path, (err: Error) => err ? reject(err) : resolve(path))
  })
}
