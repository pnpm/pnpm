import { spawnSync } from 'node:child_process'
import os from 'node:os'
import path from 'node:path'

export function getHomedir (env: NodeJS.ProcessEnv = process.env, platform: string = process.platform): string {
  if (env.SUDO_USER && env.SUDO_USER !== 'root') {
    if (platform === 'linux') {
      try {
        // cspell:disable-next-line
        const result = spawnSync('getent', ['passwd', env.SUDO_USER], { encoding: 'utf8' })
        if (result.status === 0 && result.stdout) {
          const parts = result.stdout.split(':')
          if (parts.length >= 6 && parts[5]) {
            return parts[5].trim()
          }
        }
      } catch {}
      return path.join('/home', env.SUDO_USER)
    }
    if (platform === 'darwin') {
      try {
        const result = spawnSync('dscl', ['.', '-read', `/Users/${env.SUDO_USER}`, 'NFSHomeDirectory'], { encoding: 'utf8' })
        if (result.status === 0 && result.stdout) {
          const match = result.stdout.match(/NFSHomeDirectory:\s*(.+)/)
          if (match && match[1]) {
            return match[1].trim()
          }
        }
      } catch {}
      return path.join('/Users', env.SUDO_USER)
    }
  }
  return os.homedir()
}
