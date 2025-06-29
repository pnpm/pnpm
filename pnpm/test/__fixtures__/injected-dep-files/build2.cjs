const fs = require('node:fs')
fs.writeFileSync('created-by-build2.txt', __filename)
