const fs = require('fs')
const path = require('path')

fs.appendFileSync(path.join(__dirname, 'empty-file.txt'), 'hello', 'utf8')
