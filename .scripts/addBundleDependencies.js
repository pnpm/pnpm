'use strict'
const pkg = require('../package.json')
const writePkgSync = require('./utils/writePkgSync')

pkg.name = pkg.name === 'pnpm' ? '@pnpm/bundled' : `${pkg.name}-bundled`
pkg.bundleDependencies = Object.keys(pkg.dependencies)

writePkgSync(pkg)
