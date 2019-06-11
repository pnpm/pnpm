import { findWorkspacePrefix } from '@pnpm/config'
import npx = require('@zkochan/libnpx/index')
import path = require('path')

const PNPM_PATH = path.join(__dirname, 'pnpm.js')

; (async () => {
  const workspaceRoot = await findWorkspacePrefix(process.cwd())
  if (workspaceRoot) {
    process.env.PATH = `${path.join(workspaceRoot, 'node_modules/.bin')}${path.delimiter}${process.env.PATH}`
  }
  npx({
    ...npx.parseArgs(process.argv, PNPM_PATH),
    installerStdio: 'inherit',
  })
})()
