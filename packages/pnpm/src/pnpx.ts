import path from 'path'
import findWorkspaceDir from '@pnpm/find-workspace-dir'
import npx from '@zkochan/libnpx/index'
import PATH from 'path-name'

const PNPM_PATH = path.join(__dirname, 'pnpm.cjs')

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
