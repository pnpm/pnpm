'use strict'
const pkg = require('../package.json')
const fs = require('fs')
const path = require('path')
const EOL = require('os').EOL

pkg.bundleDependencies = Object.keys(pkg.dependencies)

fs.writeFileSync(path.join(__dirname, '..', 'package.json'), JSON.stringify(pkg, null, 2) + EOL, 'utf8')
