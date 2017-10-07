'use strict'
const pkg = require('../package.json')
const writePnpmPkgSync = require('./utils/writePnpmPkgSync')

pkg.name = pkg.notBundledName
delete pkg.notBundledName
delete pkg.bundleDependencies

writePnpmPkgSync(pkg)
