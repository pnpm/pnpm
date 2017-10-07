'use strict'
const writePkgSync = require('./writePkgSync')
const path = require('path')

module.exports = function (pkg) {
  writePkgSync(path.join(__dirname, '..', '..', 'package.json'), pkg)
}
