'use strict'
const fs = require('fs')

fs.writeFileSync('generated.js', 'module.exports = function () {}\n', 'utf8')
