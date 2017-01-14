import mkdirp = require('mkdirp')
import logger from 'pnpm-logger'

const mkdirpLogger = logger('mkdirp')

/**
 * mkdir -p as a promise.
 */
export default function (path: string) {
  return new Promise((resolve, reject) => {
    mkdirpLogger.debug(path)
    mkdirp(path, (err: Error) => err ? reject(err) : resolve(path))
  })
}
