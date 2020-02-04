import findWorkspaceDir from '@pnpm/find-workspace-dir'
import npx = require('@zkochan/libnpx/index')
import path = require('path')

const PNPM_PATH = path.join(__dirname, '../../bin/pnpm.js')

// tslint:disable-next-line: no-floating-promises
; (async () => {
  const workspaceRoot = await findWorkspaceDir(process.cwd())
  if (workspaceRoot) {
    process.env.PATH = `${path.join(workspaceRoot, 'node_modules/.bin')}${path.delimiter}${process.env.PATH}`
  }
  npx({
    ...npx.parseArgs(process.argv, PNPM_PATH),
    installerStdio: 'inherit',
  })
})()
