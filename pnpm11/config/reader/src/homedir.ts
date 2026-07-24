import { spawnSync } from 'node:child_process'
import os from 'node:os'

import { PnpmError } from '@pnpm/error'

const sudoHomedirCache = new Map<string, string>()

export function getHomedir (env: NodeJS.ProcessEnv = process.env, platform: string = process.platform): string {
  if (env.SUDO_USER && env.SUDO_USER !== 'root') {
    const cacheKey = `${platform}:${env.SUDO_USER}`
    if (sudoHomedirCache.has(cacheKey)) {
      return sudoHomedirCache.get(cacheKey)!
    }

    if (platform === 'linux') {
      // cspell:disable-next-line
      const result = spawnSync('getent', ['passwd', env.SUDO_USER], { encoding: 'utf8' })
      if (result.error) {
        throw new PnpmError('SUDO_HOME_DIR_RESOLUTION', `Failed to resolve home directory for SUDO_USER '${env.SUDO_USER}': ${result.error.message}`, { cause: result.error })
      }
      if (result.status === 0 && result.stdout) {
        const parts = result.stdout.split(':')
        if (parts.length >= 6) {
          const homedir = parts[5].trim()
          sudoHomedirCache.set(cacheKey, homedir)
          return homedir
        }
      }
      // cspell:disable-next-line
      throw new PnpmError('SUDO_HOME_DIR_RESOLUTION', `Failed to resolve home directory for SUDO_USER '${env.SUDO_USER}' via getent.`)
    }
    if (platform === 'darwin') {
      const result = spawnSync('dscl', ['.', '-read', `/Users/${env.SUDO_USER}`, 'NFSHomeDirectory'], { encoding: 'utf8' })
      if (result.error) {
        throw new PnpmError('SUDO_HOME_DIR_RESOLUTION', `Failed to resolve home directory for SUDO_USER '${env.SUDO_USER}': ${result.error.message}`, { cause: result.error })
      }
      if (result.status === 0 && result.stdout) {
        const match = result.stdout.match(/NFSHomeDirectory:\s*(.+)/)
        if (match) {
          const homedir = match[1].trim()
          sudoHomedirCache.set(cacheKey, homedir)
          return homedir
        }
      }
      throw new PnpmError('SUDO_HOME_DIR_RESOLUTION', `Failed to resolve home directory for SUDO_USER '${env.SUDO_USER}' via dscl.`)
    }
  }
  return os.homedir()
}
