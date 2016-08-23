'use strict'
const osHomedir = require('os-homedir')
const path = require('path')

module.exports = function expandTilde (filepath) {
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
