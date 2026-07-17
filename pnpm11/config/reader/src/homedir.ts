import os from 'node:os'
import path from 'node:path'

export function getHomedir (env: NodeJS.ProcessEnv = process.env): string {
  if (env.SUDO_USER) {
    if (process.platform === 'linux') {
      return path.join('/home', env.SUDO_USER)
    }
    if (process.platform === 'darwin') {
      return path.join('/Users', env.SUDO_USER)
    }
  }
  return os.homedir()
}
