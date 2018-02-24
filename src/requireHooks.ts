import logger from '@pnpm/logger'
import requirePnpmfile from './requirePnpmfile'

export default function requireHooks (prefix: string) {
  const pnpmFile = requirePnpmfile(prefix)
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
