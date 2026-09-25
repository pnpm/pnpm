'use strict'
const fs = require('fs')

fs.writeFileSync('touch.txt', JSON.stringify(process.argv.slice(2)), 'utf8')
