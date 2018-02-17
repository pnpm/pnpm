'use strict'
const pkg = require('../package.json')
const writePnpmPkgSync = require('./utils/writePnpmPkgSync')

pkg.bundleDependencies = Object.keys(pkg.dependencies)

writePnpmPkgSync(pkg)
