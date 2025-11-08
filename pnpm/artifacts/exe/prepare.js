import fs from 'fs'
import path from 'path'

const pnpmCli = path.join(import.meta.dirname, 'pnpm')
fs.unlinkSync(pnpmCli)
fs.writeFileSync(pnpmCli, 'This file intentionally left blank', 'utf8')
