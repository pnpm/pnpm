'use strict'
const install = require('../api/install')

/*
 * Perform installation.
 *
 *     installCmd([ 'lodash', 'foo' ], { quiet: true })
 */

function installCmd (input, opts) {
  return install(input, opts)
}

module.exports = installCmd
