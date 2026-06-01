const fs = require('fs')

fs.writeFileSync('shell-input.json', JSON.stringify(process.argv.slice(2)))
