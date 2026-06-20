import os from 'node:os'
import path from 'node:path'

export const npmDefaults = {
  registry: 'https://registry.npmjs.org/',
  'package-lock': true,
  'unsafe-perm': process.platform === 'win32' ||
    process.platform === 'cygwin' ||
    !(process.getuid && process.setuid && process.getgid && process.setgid) ||
    process.getuid!() !== 0,
  userconfig: path.resolve(os.homedir(), '.npmrc'),
  maxsockets: 50,
}
