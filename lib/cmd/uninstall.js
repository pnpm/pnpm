'use strict'
const uninstall = require('../api/uninstall')

function uninstallCmd (input, opts) {
  return uninstall(input, opts)
}

module.exports = uninstallCmd
