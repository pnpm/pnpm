'use strict'
const fs = require('fs')

fs.writeFileSync(process.argv[2] + '.js', 'module.exports = function () {}\n', 'utf8')
