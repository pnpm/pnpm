import os from 'node:os'
import path from 'node:path'

function getGlobalPrefix (): string {
  if (process.env.PREFIX) {
    return process.env.PREFIX
  }
  if (process.platform === 'win32') {
    return path.dirname(process.execPath)
  }
  let prefix = path.dirname(path.dirname(process.execPath))
  if (process.env.DESTDIR) { // cspell:disable-line
    prefix = path.join(process.env.DESTDIR, prefix) // cspell:disable-line
  }
  return prefix
}

const home = os.homedir()

export const npmDefaults = {
  registry: 'https://registry.npmjs.org/',
  'package-lock': true,
  'unsafe-perm': process.platform === 'win32' ||
    process.platform === 'cygwin' ||
    !(process.getuid && process.setuid && process.getgid && process.setgid) ||
    process.getuid!() !== 0,
  userconfig: path.resolve(home, '.npmrc'),
  globalconfig: path.resolve(getGlobalPrefix(), 'etc', 'npmrc'),
  maxsockets: 50,
}
