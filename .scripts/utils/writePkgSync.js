'use strict'
const fs = require('fs')
const path = require('path')
const EOL = require('os').EOL

module.exports = function (pkg) {
  fs.writeFileSync(path.join(__dirname, '..', '..', 'package.json'), JSON.stringify(pkg, null, 2) + EOL, 'utf8')
}
