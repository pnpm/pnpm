const fs = require('fs')
const path = require('path')

fs.writeFileSync(path.join(__dirname, 'pnpm'), '', 'utf8')
