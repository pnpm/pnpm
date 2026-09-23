import { resolvePnpmExecPath } from '@pnpm/cli.meta'

export function makePackageManagerEnv (env: NodeJS.ProcessEnv): Record<string, string> {
  const nodeExecPath = env.NODE || process.execPath
  return {
    NODE: nodeExecPath,
    npm_node_execpath: nodeExecPath,
    npm_execpath: resolvePnpmExecPath() ?? 'pnpm',
    INIT_CWD: process.cwd(),
  }
}
