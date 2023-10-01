import { existsSync } from 'fs'
import path from 'path'
import { PnpmError } from '@pnpm/error'
import { createFetchFromRegistry, type FetchFromRegistry } from '@pnpm/fetch'
import { globalInfo } from '@pnpm/logger'
import { resolveNodeVersion } from '@pnpm/node.resolver'
import { removeBin } from '@pnpm/remove-bins'
import rimraf from '@zkochan/rimraf'
import { parseEnvSpecifier } from './parseEnvSpecifier'
import { getNodeExecPathAndTargetDir } from './utils'
import { getNodeMirror } from './getNodeMirror'
import { getNodeVersionsBaseDir, type NvmNodeCommandOptions } from './node'

export async function envRemove (opts: NvmNodeCommandOptions, params: string[]) {
  if (!opts.global) {
    throw new PnpmError('NOT_IMPLEMENTED_YET', '"pnpm env use <version>" can only be used with the "--global" option currently')
  }

  const fetch = createFetchFromRegistry(opts)
  const messages = []
  for (const version of params) {
    // eslint-disable-next-line no-await-in-loop
    messages.push(await removeNodeVersion(fetch, opts, version))
  }
  if (messages.length === 1 && messages[0] instanceof Error) throw messages[0]
  return messages.map((msg: string | Error) => msg instanceof Error ? msg.message : msg).join('\n')
}

async function removeNodeVersion (fetch: FetchFromRegistry, opts: NvmNodeCommandOptions, version: string) {
  const { releaseChannel, versionSpecifier } = parseEnvSpecifier(version)
  const nodeMirrorBaseUrl = getNodeMirror(opts.rawConfig, releaseChannel)
  const nodeVersion = await resolveNodeVersion(fetch, versionSpecifier, nodeMirrorBaseUrl)
  const nodeDir = getNodeVersionsBaseDir(opts.pnpmHomeDir)

  if (!nodeVersion) {
    throw new PnpmError('COULD_NOT_RESOLVE_NODEJS', `Couldn't find Node.js version matching ${version}`)
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

  return `Node.js ${nodeVersion as string} is removed
  ${versionDir}`
}
