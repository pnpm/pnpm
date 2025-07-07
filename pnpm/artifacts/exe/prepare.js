const fs = require('node:fs')
const path = require('node:path')

const pnpmCli = path.join(__dirname, 'pnpm')
fs.unlinkSync(pnpmCli)
fs.writeFileSync(pnpmCli, 'This file intentionally left blank', 'utf8')
