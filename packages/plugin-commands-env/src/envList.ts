import { promises as fs, existsSync } from 'fs'
import path from 'path'
import { createFetchFromRegistry } from '@pnpm/fetch'
import { resolveNodeVersions } from '@pnpm/node.resolver'
import { PnpmError } from '@pnpm/error'
import semver from 'semver'
import { getNodeMirror } from './getNodeMirror'
import { getNodeVersionsBaseDir, NvmNodeCommandOptions } from './node'
import { parseNodeEditionSpecifier } from './parseNodeEditionSpecifier'

export const listLocalVersions = async (opts: NvmNodeCommandOptions) => {
  const nodeDir = getNodeVersionsBaseDir(opts.pnpmHomeDir)
  const nodePath = path.resolve(opts.pnpmHomeDir, process.platform === 'win32' ? 'node.exe' : 'node')
  let currentNodeVersion: string | undefined
  let nodeLink: string | undefined

  if (!existsSync(nodeDir)) {
    throw new PnpmError('ENV_NO_NODE_DIRECTORY', `Couldn't find Node.js directory in ${nodeDir}`)
  }

  try {
    nodeLink = await fs.readlink(nodePath)
  } catch (err) {
    nodeLink = undefined
  }

  const nodeVersionDirs = await fs.readdir(nodeDir)
  const nodeVersionList = nodeVersionDirs.filter(nodeVersion => {
    const nodeSrc = path.join(nodeDir, nodeVersion, process.platform === 'win32' ? 'node.exe' : 'bin/node')
    const nodeVersionDir = path.join(nodeDir, nodeVersion)
    if (nodeLink?.includes(nodeVersionDir)) {
      currentNodeVersion = nodeVersion
    }
    return semver.valid(nodeVersion) && existsSync(nodeSrc)
  })
  return { currentNodeVersion, nodeVersionList }
}

export const listRemoteVersions = async (opts: NvmNodeCommandOptions, versionSpec?: string) => {
  const fetch = createFetchFromRegistry(opts)
  const { releaseChannel, versionSpecifier } = parseNodeEditionSpecifier(versionSpec ?? '')
  const nodeMirrorBaseUrl = getNodeMirror(opts.rawConfig, releaseChannel)
  const nodeVersionList = await resolveNodeVersions(fetch, {
    versionSpec: versionSpecifier,
    nodeMirrorBaseUrl,
  })
  return nodeVersionList
}
