import path from 'path'
import { existsSync } from 'fs'

import rimraf from '@zkochan/rimraf'
import { PnpmError } from '@pnpm/error'
import { removeBin } from '@pnpm/remove-bins'
import { globalInfo, logger } from '@pnpm/logger'

import { getNodeExecPathAndTargetDir } from './utils'
import { getNodeVersion } from './downloadNodeVersion'
import { getNodeVersionsBaseDir, type NvmNodeCommandOptions } from './node'

export async function envRemove(opts: NvmNodeCommandOptions, params: string[]): Promise<{
  exitCode: number;
}> {
  if (!opts.global) {
    throw new PnpmError(
      'NOT_IMPLEMENTED_YET',
      '"pnpm env remove <version>" can only be used with the "--global" option currently'
    )
  }

  let failed = false

  for (const version of params) {
    // eslint-disable-next-line no-await-in-loop
    const err = await removeNodeVersion(opts, version)

    if (err) {
      logger.error(err)
      failed = true
    }
  }

  return { exitCode: failed ? 1 : 0 }
}

async function removeNodeVersion(
  opts: NvmNodeCommandOptions,
  version: string
): Promise<Error | undefined> {
  const { nodeVersion } = await getNodeVersion(opts, version)

  const nodeDir = getNodeVersionsBaseDir(opts.pnpmHomeDir)

  if (!nodeVersion) {
    return new PnpmError(
      'COULD_NOT_RESOLVE_NODEJS',
      `Couldn't find Node.js version matching ${version}`
    )
  }

  const versionDir = path.resolve(nodeDir, nodeVersion)

  if (!existsSync(versionDir)) {
    return new PnpmError(
      'ENV_NO_NODE_DIRECTORY',
      `Couldn't find Node.js directory in ${versionDir}`
    )
  }

  const { nodePath, nodeLink } = await getNodeExecPathAndTargetDir(
    opts.pnpmHomeDir
  )

  if (nodeLink?.includes(versionDir)) {
    globalInfo(
      `Node.js ${nodeVersion as string} was detected as the default one, removing ...`
    )

    const npmPath = path.resolve(opts.pnpmHomeDir, 'npm')
    const npxPath = path.resolve(opts.pnpmHomeDir, 'npx')

    try {
      await Promise.all([
        removeBin(nodePath),
        removeBin(npmPath),
        removeBin(npxPath),
      ])
    } catch (err: any) { // eslint-disable-line
      if (err.code !== 'ENOENT') return err
    }
  }

  await rimraf(versionDir)

  globalInfo(`Node.js ${nodeVersion as string} was removed ${versionDir}`)

  return undefined
}
