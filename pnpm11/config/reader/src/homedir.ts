import { spawnSync } from 'node:child_process'
import os from 'node:os'

import { PnpmError } from '@pnpm/error'

const GETENT = '/usr/bin/getent'
const DSCL = '/usr/bin/dscl'

const sudoHomedirCache = new Map<string, string>()

function resolveHomedirViaGetent (sudoUser: string): string {
  const result = spawnSync(GETENT, ['passwd', sudoUser], { encoding: 'utf8' })
  if (result.error) {
    throw new PnpmError('SUDO_HOME_DIR_RESOLUTION', `Failed to resolve home directory for SUDO_USER '${sudoUser}'`, { cause: result.error })
  }
  if (result.status === 0 && result.stdout) {
    const parts = result.stdout.split(':')
    const homedir = parts.length >= 6 ? parts[5].trim() : ''
    if (homedir) {
      return homedir
    }
  }
  throw new PnpmError('SUDO_HOME_DIR_RESOLUTION', `Failed to resolve home directory for SUDO_USER '${sudoUser}' via ${GETENT}.`)
}

function resolveHomedirViaDscl (sudoUser: string): string {
  const result = spawnSync(DSCL, ['.', '-read', `/Users/${sudoUser}`, 'NFSHomeDirectory'], { encoding: 'utf8' })
  if (result.error) {
    throw new PnpmError('SUDO_HOME_DIR_RESOLUTION', `Failed to resolve home directory for SUDO_USER '${sudoUser}'`, { cause: result.error })
  }
  if (result.status === 0 && result.stdout) {
    const match = result.stdout.match(/NFSHomeDirectory:\s*(.+)/)
    if (match) {
      return match[1].trim()
    }
  }
  throw new PnpmError('SUDO_HOME_DIR_RESOLUTION', `Failed to resolve home directory for SUDO_USER '${sudoUser}' via ${DSCL}.`)
}

export function getHomedir (env: NodeJS.ProcessEnv = process.env, platform: string = process.platform): string {
  if (env.SUDO_USER && env.SUDO_USER !== 'root' && typeof process.getuid === 'function' && process.getuid() === 0) {
    const cacheKey = `${platform}:${env.SUDO_USER}`
    if (sudoHomedirCache.has(cacheKey)) {
      return sudoHomedirCache.get(cacheKey)!
    }

    if (platform === 'linux' || platform === 'freebsd') {
      const homedir = resolveHomedirViaGetent(env.SUDO_USER)
      sudoHomedirCache.set(cacheKey, homedir)
      return homedir
    }
    if (platform === 'darwin') {
      const homedir = resolveHomedirViaDscl(env.SUDO_USER)
      sudoHomedirCache.set(cacheKey, homedir)
      return homedir
    }
  }
  return os.homedir()
}
