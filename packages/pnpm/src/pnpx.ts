import findWorkspaceDir from '@pnpm/find-workspace-dir'
import path = require('path')
import npx = require('@zkochan/libnpx/index')
import PATH = require('path-name')

const PNPM_PATH = path.join(__dirname, 'pnpm.js')

// eslint-disable-next-line @typescript-eslint/no-floating-promises
; (async () => {
  const workspaceRoot = await findWorkspaceDir(process.cwd())
  if (workspaceRoot) {
    process.env[PATH] = `${path.join(workspaceRoot, 'node_modules/.bin')}${path.delimiter}${process.env[PATH] ?? ''}`
  }
  npx({
    ...npx.parseArgs(process.argv, PNPM_PATH),
    installerStdio: 'inherit',
  })
})()
