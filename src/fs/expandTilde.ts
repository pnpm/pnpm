import osHomedir = require('os-homedir')
import path = require('path')

export default function expandTilde (filepath: string, cwd?: string) {
  const home = getHomedir()

  if (isHomepath(filepath)) {
    return path.join(home, filepath.substr(2))
  }
  if (path.isAbsolute(filepath)) {
    return filepath
  }
  if (cwd) {
    return path.join(cwd, filepath)
  }
  return path.resolve(filepath)
}

function getHomedir () {
  const home = osHomedir()
  if (!home) throw new Error('Could not find the homedir')
  return home
}

export function isHomepath (filepath: string) {
  return filepath.indexOf('~/') === 0 || filepath.indexOf('~\\') === 0
}
