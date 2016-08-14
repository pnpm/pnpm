'use strict'
const install = require('./lib/api/install')
const uninstall = require('./lib/api/install')

module.exports = {
  install,
  installPkgDeps (opts) {
    return install({}, opts)
  },
  uninstall
}
