const fs = require('fs')
const path = require('path')

const pnpmCli = path.join(__dirname, 'pnpm')
fs.unlinkSync(pnpmCli)
fs.writeFileSync(pnpmCli, '', 'utf8')
