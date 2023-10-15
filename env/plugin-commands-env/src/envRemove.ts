/* eslint-disable no-await-in-loop */
import { existsSync } from 'fs'
import path from 'path'
import { PnpmError } from '@pnpm/error'
import { logger, globalInfo } from '@pnpm/logger'
import { removeBin } from '@pnpm/remove-bins'
import rimraf from '@zkochan/rimraf'
import { getNodeExecPathAndTargetDir } from './utils'
import { getNodeVersionsBaseDir, type NvmNodeCommandOptions } from './node'
import { getNodeVersion } from './downloadNodeVersion'

export async function envRemove (opts: NvmNodeCommandOptions, params: string[]) {
  if (!opts.global) {
    throw new PnpmError('NOT_IMPLEMENTED_YET', '"pnpm env use <version>" can only be used with the "--global" option currently')
  }

  let failed = false
  for (const version of params) {
    const err = await removeNodeVersion(opts, version)
    if (err) {
      logger.error(err)
      failed = true
    }
  }
  return { exitCode: failed ? 1 : 0 }
}

async function removeNodeVersion (opts: NvmNodeCommandOptions, version: string): Promise<Error | undefined> {
  const { nodeVersion } = await getNodeVersion(opts, version)
  const nodeDir = getNodeVersionsBaseDir(opts.pnpmHomeDir)

  if (!nodeVersion) {
    return new PnpmError('COULD_NOT_RESOLVE_NODEJS', `Couldn't find Node.js version matching ${version}`)
  }

  const versionDir = path.resolve(nodeDir, nodeVersion)

  if (!existsSync(versionDir)) {
    return new PnpmError('ENV_NO_NODE_DIRECTORY', `Couldn't find Node.js directory in ${versionDir}`)
  }

  const { nodePath, nodeLink } = await getNodeExecPathAndTargetDir(opts.pnpmHomeDir)

  if (nodeLink?.includes(versionDir)) {
    globalInfo(`Node.js version ${nodeVersion as string} was detected as the default one, removing ...`)

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

  globalInfo(`Node.js ${nodeVersion as string} was removed
${versionDir}`)
  return undefined
}
