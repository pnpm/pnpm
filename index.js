'use strict'
const install = require('./lib/api/install')
const installPkgDeps = require('./lib/api/install_pkg_deps')
const uninstall = require('./lib/api/uninstall')
const link = require('./lib/api/link')

module.exports = Object.assign({
  install,
  installPkgDeps,
  uninstall
}, link)
