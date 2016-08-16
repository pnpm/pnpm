'use strict'
const install = require('./lib/api/install')
const uninstall = require('./lib/api/uninstall')

module.exports = {
  install,
  installPkgDeps (opts) {
    return install({}, opts)
  },
  uninstall
}
