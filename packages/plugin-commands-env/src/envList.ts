import { promises as fs, existsSync } from 'fs'
import path from 'path'
import { createFetchFromRegistry } from '@pnpm/fetch'
import { resolveNodeVersions } from '@pnpm/node.resolver'
import { PnpmError } from '@pnpm/error'
import semver from 'semver'
import { getNodeMirror } from './getNodeMirror'
import { getNodeVersionsBaseDir, NvmNodeCommandOptions } from './node'
import { parseNodeEditionSpecifier } from './parseNodeEditionSpecifier'
import { getNodeExecPathAndTargetDir, getNodeExecPathInNodeDir } from './utils'

export async function listLocalVersions (opts: NvmNodeCommandOptions) {
  const nodeBaseDir = getNodeVersionsBaseDir(opts.pnpmHomeDir)
  if (!existsSync(nodeBaseDir)) {
    throw new PnpmError('ENV_NO_NODE_DIRECTORY', `Couldn't find Node.js directory in ${nodeBaseDir}`)
  }
  const { nodeLink } = await getNodeExecPathAndTargetDir(opts.pnpmHomeDir)
  const nodeVersionDirs = await fs.readdir(nodeBaseDir)
  return nodeVersionDirs.reduce(({ currentVersion, versions }, nodeVersion) => {
    const nodeVersionDir = path.join(nodeBaseDir, nodeVersion)
    const nodeExec = getNodeExecPathInNodeDir(nodeVersionDir)
    if (nodeLink?.startsWith(nodeVersionDir)) {
      currentVersion = nodeVersion
    }
    if (semver.valid(nodeVersion) && existsSync(nodeExec)) {
      versions.push(nodeVersion)
    }
    return { currentVersion, versions }
  }, { currentVersion: undefined as string | undefined, versions: [] as string[] })
}

export async function listRemoteVersions (opts: NvmNodeCommandOptions, versionSpec?: string) {
  const fetch = createFetchFromRegistry(opts)
  const { releaseChannel, versionSpecifier } = parseNodeEditionSpecifier(versionSpec ?? '')
  const nodeMirrorBaseUrl = getNodeMirror(opts.rawConfig, releaseChannel)
  const nodeVersionList = await resolveNodeVersions(fetch, versionSpecifier, nodeMirrorBaseUrl)
  return nodeVersionList
}
