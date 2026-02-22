import { createFetchFromRegistry } from '@pnpm/fetch'
import { resolveNodeVersions, parseEnvSpecifier, getNodeMirror } from '@pnpm/node.resolver'
import { type NvmNodeCommandOptions } from './node.js'

export async function envList (opts: NvmNodeCommandOptions, params: string[]): Promise<string> {
  const nodeVersionList = await listRemoteVersions(opts, params[0])
  // Make the newest version located at the end of the output
  return nodeVersionList.reverse().join('\n')
}

async function listRemoteVersions (opts: NvmNodeCommandOptions, versionSpec?: string): Promise<string[]> {
  const fetch = createFetchFromRegistry(opts)
  const { releaseChannel, versionSpecifier } = parseEnvSpecifier(versionSpec ?? '')
  const nodeMirrorBaseUrl = getNodeMirror(opts.rawConfig, releaseChannel)
  return resolveNodeVersions(fetch, versionSpecifier, nodeMirrorBaseUrl)
}
