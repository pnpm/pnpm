import { spawnSync } from 'node:child_process'
import os from 'node:os'

export function getHomedir (env: NodeJS.ProcessEnv = process.env, platform: string = process.platform): string {
  if (env.SUDO_USER && env.SUDO_USER !== 'root') {
    if (platform === 'linux') {
      try {
        // cspell:disable-next-line
        const result = spawnSync('getent', ['passwd', env.SUDO_USER], { encoding: 'utf8' })
        if (result.status === 0 && result.stdout) {
          const parts = result.stdout.split(':')
          if (parts.length >= 6) {
            return parts[5]
          }
        }
        // cspell:disable-next-line
        throw new Error(`Failed to resolve home directory for SUDO_USER '${env.SUDO_USER}' via getent.`)
      } catch (err: any) { // eslint-disable-line
        throw new Error(`Failed to resolve home directory for SUDO_USER '${env.SUDO_USER}': ${err}`, { cause: err })
      }
    }
    if (platform === 'darwin') {
      try {
        const result = spawnSync('dscl', ['.', '-read', `/Users/${env.SUDO_USER}`, 'NFSHomeDirectory'], { encoding: 'utf8' })
        if (result.status === 0 && result.stdout) {
          const match = result.stdout.match(/NFSHomeDirectory:\s*(.+)/)
          if (match) {
            return match[1].trim()
          }
        }
        throw new Error(`Failed to resolve home directory for SUDO_USER '${env.SUDO_USER}' via dscl.`)
      } catch (err: any) { // eslint-disable-line
        throw new Error(`Failed to resolve home directory for SUDO_USER '${env.SUDO_USER}': ${err}`, { cause: err })
      }
    }
  }
  return os.homedir()
}
