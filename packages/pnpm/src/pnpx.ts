import path from 'path'
import packageManager from '@pnpm/cli-meta'
import findWorkspaceDir from '@pnpm/find-workspace-dir'
import storePath from '@pnpm/store-path'
import npx from '@zkochan/libnpx/index'
import PATH from 'path-name'

const PNPM_PATH = path.join(__dirname, 'pnpm.cjs')

// eslint-disable-next-line @typescript-eslint/no-floating-promises
; (async () => {
  const workspaceRoot = await findWorkspaceDir(process.cwd())
  process.env['npm_config_user_agent'] = `${packageManager.name}/${packageManager.version} npm/? node/${process.version} ${process.platform} ${process.arch}`
  if (workspaceRoot) {
    process.env[PATH] = `${path.join(workspaceRoot, 'node_modules/.bin')}${path.delimiter}${process.env[PATH] ?? ''}`
  }
  npx({
    ...npx.parseArgs(process.argv, PNPM_PATH),
    cache: path.join(await storePath(process.cwd(), '~/.pnpm-store'), 'tmp'),
    installerStdio: 'inherit',
  })
})()
