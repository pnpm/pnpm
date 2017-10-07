'use strict'
const pkg = require('../package.json')

if (pkg.name.endsWith('/bundled') || pkg.name.endsWith('-bundled')) {
  throw new Error(`Cannot publish a package called ${pkg.name}`)
}

if (pkg.bundleDependencies || pkg.bundledDependencies) {
  throw new Error('Cannot publish a package with bundled dependencies')
}
