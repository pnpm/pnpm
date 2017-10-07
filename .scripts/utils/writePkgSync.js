'use strict'
const fs = require('fs')
const path = require('path')
const EOL = require('os').EOL

module.exports = function (pkgPath, pkg) {
  fs.writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + EOL, 'utf8')
}
