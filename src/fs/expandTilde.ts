import osHomedir = require('os-homedir')
import path = require('path')

export default function expandTilde (filepath: string) {
  const home = getHomedir()

  if (filepath.indexOf('~/') !== 0) {
    return filepath
  }
  return path.resolve(home, filepath.substr(2))
}

function getHomedir () {
  const home = osHomedir()
  if (!home) throw new Error('Could not find the homedir')
  return home
}
