'use strict'
const pnpmPkg = require('../package.json')
const selfInstallerPkg = require('./self-installer/package.json')
const writePkgSync = require('./utils/writePkgSync')
const path = require('path')

selfInstallerPkg.version = pnpmPkg.version

writePkgSync(path.join(__dirname, 'self-installer', 'package.json'), selfInstallerPkg)
