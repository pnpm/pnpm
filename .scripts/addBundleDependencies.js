'use strict'
const pkg = require('../package.json')
const writePnpmPkgSync = require('./utils/writePnpmPkgSync')

pkg.notBundledName = pkg.name
pkg.name = pkg.bundledName
pkg.bundleDependencies = Object.keys(pkg.dependencies)

writePnpmPkgSync(pkg)
