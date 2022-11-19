import { existsSync } from 'fs'
import path from 'path'
import { PnpmError } from '@pnpm/error'
import { createFetchFromRegistry } from '@pnpm/fetch'
import { globalInfo } from '@pnpm/logger'
import { resolveNodeVersion } from '@pnpm/node.resolver'
import { removeBin } from '@pnpm/remove-bins'
import rimraf from '@zkochan/rimraf'
import { parseNodeEditionSpecifier } from './parseNodeEditionSpecifier'
import { getNodeExecPathAndTargetDir } from './utils'
import { getNodeMirror } from './getNodeMirror'
import { getNodeVersionsBaseDir, NvmNodeCommandOptions } from './node'

export async function envRemove (opts: NvmNodeCommandOptions, params: string[]) {
  if (!opts.global) {
    throw new PnpmError('NOT_IMPLEMENTED_YET', '"pnpm env use <version>" can only be used with the "--global" option currently')
  }

  const fetch = createFetchFromRegistry(opts)
  const { releaseChannel, versionSpecifier } = parseNodeEditionSpecifier(params[0])
  const nodeMirrorBaseUrl = getNodeMirror(opts.rawConfig, releaseChannel)
  const nodeVersion = await resolveNodeVersion(fetch, versionSpecifier, nodeMirrorBaseUrl)
  const nodeDir = getNodeVersionsBaseDir(opts.pnpmHomeDir)

  if (!nodeVersion) {
    throw new PnpmError('COULD_NOT_RESOLVE_NODEJS', `Couldn't find Node.js version matching ${params[0]}`)
  }

  const versionDir = path.resolve(nodeDir, nodeVersion)

  if (!existsSync(versionDir)) {
    throw new PnpmError('ENV_NO_NODE_DIRECTORY', `Couldn't find Node.js directory in ${versionDir}`)
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
      if (err.code !== 'ENOENT') throw err
    }
  }

  await rimraf(versionDir)

  return `Node.js ${nodeVersion as string} is removed
${versionDir}`
}
