import logger from '@pnpm/logger'
import path = require('path')
import pathAbsolute = require('path-absolute')
import requirePnpmfile from './requirePnpmfile'

export default function requireHooks (prefix: string, customPnpmfileLocation: string | undefined) {
  const pnpmFile = requirePnpmfile(path.join(prefix, 'pnpmfile.js'))
    || customPnpmfileLocation && requirePnpmfile(pathAbsolute(customPnpmfileLocation, prefix))
  const hooks = pnpmFile && pnpmFile.hooks
  if (!hooks) return {}
  if (hooks.readPackage) {
    if (typeof hooks.readPackage !== 'function') {
      throw new TypeError('hooks.readPackage should be a function')
    }
    logger.info('readPackage hook is declared. Manifests of dependencies might get overridden')
  }
  return hooks
}
