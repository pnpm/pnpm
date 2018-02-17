'use strict'
const pkg = require('../package.json')
const writePnpmPkgSync = require('./utils/writePnpmPkgSync')

delete pkg.bundleDependencies

writePnpmPkgSync(pkg)
