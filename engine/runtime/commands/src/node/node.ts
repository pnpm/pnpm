import { existsSync } from 'node:fs'
import path from 'node:path'

import { docsUrl } from '@pnpm/cli.utils'
import type { Config } from '@pnpm/config.reader'
import crossSpawn from 'cross-spawn'
import { renderHelp } from 'render-help'

const IS_WINDOWS = process.platform === 'win32'
const NODE_PKG_BIN_REL = IS_WINDOWS ? 'node.exe' : path.join('bin', 'node')

export type NodeCommandOptions = Pick<Config,
| 'dir'
| 'pnpmHomeDir'
> & Partial<Pick<Config,
| 'globalBinDir'
| 'modulesDir'
| 'workspaceDir'
>>

export const commandNames = ['node']

// A project script named "node" takes precedence — `pnpm node` then behaves
// like `pnpm run node` (preserving prior fallback behavior for users who
// already have such a script).
export const overridableByScript = true

export const skipPackageManagerCheck = true

export function rcOptionsTypes (): Record<string, unknown> {
  return {}
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {}
}

export function help (): string {
  return renderHelp({
    description: 'Runs Node.js using the version managed by pnpm, ignoring any "node" \
binary that may be on PATH. The runtime is resolved from the project (when \
"devEngines.runtime" is installed) or from pnpm\'s global runtime install \
(`pnpm runtime set node <version> -g`). Falls back to "node" on PATH only as a \
last resort.',
    descriptionLists: [],
    url: docsUrl('node'),
    usages: [
      'pnpm node -v',
      'pnpm node script.js',
      'pnpm node -e "console.log(1)"',
    ],
  })
}

export async function handler (
  opts: NodeCommandOptions,
  params: string[]
): Promise<{ exitCode: number }> {
  const nodePath = findNodeBinary(opts)
  const { status, signal, error } = crossSpawn.sync(nodePath ?? 'node', params, {
    stdio: 'inherit',
  })
  if (error) throw error
  if (signal) {
    // Forward the terminating signal to the parent so callers don't see a
    // successful exit when the child was killed.
    process.kill(process.pid, signal)
    return { exitCode: 1 }
  }
  return { exitCode: status ?? 0 }
}

function findNodeBinary (opts: NodeCommandOptions): string | undefined {
  // 1. Project-level runtime: look in the project's own node_modules, then
  //    the workspace root's (pnpm hoists the runtime up to the workspace
  //    when devEngines.runtime is set on the workspace root).
  const modulesDir = opts.modulesDir ?? 'node_modules'
  const candidates = [path.join(opts.dir, modulesDir)]
  if (opts.workspaceDir && opts.workspaceDir !== opts.dir) {
    candidates.push(path.join(opts.workspaceDir, modulesDir))
  }
  for (const nodeModulesDir of candidates) {
    const projectBin = path.join(nodeModulesDir, 'node', NODE_PKG_BIN_REL)
    if (existsSync(projectBin)) return projectBin
  }

  // 2. Global pnpm runtime: `pnpm runtime set node X -g` hardlinks/copies
  //    `node.exe` (Windows) or symlinks `node` (POSIX) into the global bin
  //    directory. That binary works regardless of whether the dir is on PATH.
  const globalBin = opts.globalBinDir ?? path.join(opts.pnpmHomeDir, 'bin')
  const globalNode = path.join(globalBin, IS_WINDOWS ? 'node.exe' : 'node')
  if (existsSync(globalNode)) return globalNode

  // 3. Fall back to PATH (cross-spawn will resolve "node").
  return undefined
}
