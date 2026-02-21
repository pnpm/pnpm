import { resolveNodeVersion, parseEnvSpecifier, getNodeMirror } from '@pnpm/node.resolver'
import { type NvmNodeCommandOptions } from './node.js'
import { createFetchFromRegistry } from '@pnpm/fetch'

export interface GetNodeVersionResult {
  nodeVersion: string | null
  nodeMirrorBaseUrl: string
  releaseChannel: string
  versionSpecifier: string
}

export async function getNodeVersion (opts: NvmNodeCommandOptions, envSpecifier: string): Promise<GetNodeVersionResult> {
  const fetch = createFetchFromRegistry(opts)
  const { releaseChannel, versionSpecifier } = parseEnvSpecifier(envSpecifier)
  const nodeMirrorBaseUrl = getNodeMirror(opts.rawConfig, releaseChannel)
  const nodeVersion = await resolveNodeVersion(fetch, versionSpecifier, nodeMirrorBaseUrl)
  return { nodeVersion, nodeMirrorBaseUrl, releaseChannel, versionSpecifier }
}
