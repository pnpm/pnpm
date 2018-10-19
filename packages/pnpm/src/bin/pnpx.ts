import npx = require('@zkochan/libnpx/index')
import path = require('path')

const PNPM_PATH = path.join(__dirname, 'pnpm.js')

const npxOpts = Object.assign({}, npx.parseArgs(process.argv, PNPM_PATH), {
  installerStdio: 'inherit',
})
npx(npxOpts)
