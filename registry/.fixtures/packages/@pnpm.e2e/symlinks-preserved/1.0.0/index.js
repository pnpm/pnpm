'use strict'
const fs = require('fs')

module.exports = fs.realpathSync(__filename) !== __filename
