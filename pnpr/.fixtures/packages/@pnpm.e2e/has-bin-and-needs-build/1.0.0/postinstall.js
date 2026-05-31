const path = require('path')
const fs = require('fs')

fs.renameSync(path.join(__dirname, '_bin.js'), path.join(__dirname, 'bin.js'))
