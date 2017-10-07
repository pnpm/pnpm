'use strict'
const pkg = require('../package.json')
const writePkgSync = require('./utils/writePkgSync')

pkg.name = pkg.name === '@pnpm/bundled' ? 'pnpm' : pkg.name.replace('-bundled', '')
delete pkg.bundleDependencies

writePkgSync(pkg)
