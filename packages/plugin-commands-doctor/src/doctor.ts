import { execSync } from 'child_process'
import renderHelp from 'render-help'
import { docsUrl } from '@pnpm/cli-utils'
import { logger } from '@pnpm/logger'
import { type Config } from '@pnpm/config'

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes (): Record<string, unknown> {
  return {}
}

export const shorthands = {}

export const commandNames = ['doctor']

export function help (): string {
  return renderHelp({
    description: 'Checks for known common issues.',
    url: docsUrl('doctor'),
    usages: ['pnpm doctor [options]'],
  })
}

// In Windows system exFAT drive, symlink will result in error.
function currentDriveIsExFAT (): boolean {
  try {
    const currentDrive = process.cwd().split(':')[0] + ':'
    const output = execSync(`wmic logicaldisk where "DeviceID='${currentDrive}'" get FileSystem`).toString()
    const lines = output.trim().split('\n')
    const name = lines.length > 1 ? lines[1].trim() : ''
    return name === 'exFAT'
  } catch {}
  return false
}

export async function handler (
  opts: Pick<Config, 'failedToLoadBuiltInConfig' | 'nodeLinker'>
): Promise<void> {
  const { failedToLoadBuiltInConfig, nodeLinker } = opts
  if (failedToLoadBuiltInConfig) {
    // If true, means loading npm builtin config failed. Then there may have a prefix error, related: https://github.com/pnpm/pnpm/issues/5404
    logger.warn({
      message: 'Load npm builtin configs failed. If the prefix builtin config does not work, you can use "pnpm config list" to show builtin configs. And then use "pnpm config --global set <key> <value>" to migrate configs from builtin to global.',
      prefix: process.cwd(),
    })
  }
  if (nodeLinker !== 'hoisted' && currentDriveIsExFAT()) {
    // If the node_modules is not hoisted, and the current drive is exFAT, then there may have a symlink error, related:
    logger.warn({
      message: 'The current drive is exFAT, which does not support symlinks. This may cause some packages to fail to install. You can set the node-linker to "hoisted" to avoid this issue.',
      prefix: process.cwd(),
    })
  }
}
