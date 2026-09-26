import { resolvePnpmExecPath } from '@pnpm/cli.meta'
import which from 'which'

export function makePackageManagerEnv (env: NodeJS.ProcessEnv): Record<string, string> {
  const nodeExecPath = env.NODE || process.execPath
  return {
    NODE: nodeExecPath,
    npm_node_execpath: nodeExecPath,
    npm_execpath: resolvePnpmExecPath() ?? findPnpmOnPath(),
    INIT_CWD: process.cwd(),
  }
}

/**
 * The pnpm on this process's `PATH`. It is resolved here because a script's
 * `PATH` starts with `node_modules/.bin`, where a dependency's bin named
 * `pnpm` would otherwise stand in for the package manager.
 */
function findPnpmOnPath (): string {
  return which.sync('pnpm', { path: process.env.PATH, nothrow: true }) ?? 'pnpm'
}
