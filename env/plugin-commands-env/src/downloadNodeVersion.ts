import { resolveNodeVersion } from '@pnpm/node.resolver'
import { getNodeMirror } from './getNodeMirror'
import { getNodeDir, type NvmNodeCommandOptions } from './node'
import { parseEnvSpecifier } from './parseEnvSpecifier'
import { createFetchFromRegistry } from '@pnpm/fetch'
import { globalInfo } from '@pnpm/logger'
import semver from 'semver'

export interface GetNodeVersionResult {
  nodeVersion: string | null
  nodeMirrorBaseUrl: string
  releaseChannel: string
  versionSpecifier: string
}

export async function getNodeVersion (opts: NvmNodeCommandOptions, envSpecifier: string): Promise<GetNodeVersionResult> {
  let nodeVersion
  if (semver.valid(envSpecifier)) nodeVersion = envSpecifier
  const { releaseChannel, versionSpecifier } = parseEnvSpecifier(envSpecifier)
  const nodeMirrorBaseUrl = getNodeMirror(opts.rawConfig, releaseChannel)
  nodeVersion ??= await resolveNodeVersion(createFetchFromRegistry(opts), versionSpecifier, nodeMirrorBaseUrl)
  return { nodeVersion, nodeMirrorBaseUrl, releaseChannel, versionSpecifier }
}

export interface DownloadNodeVersionResult {
  nodeVersion: string
  nodeDir: string
  nodeMirrorBaseUrl: string
}

export async function downloadNodeVersion (opts: NvmNodeCommandOptions, envSpecifier: string): Promise<DownloadNodeVersionResult | null> {
  const fetch = createFetchFromRegistry(opts)
  const { nodeVersion, nodeMirrorBaseUrl } = await getNodeVersion(opts, envSpecifier)
  if (!nodeVersion) {
    return null
  }
  const nodeDir = await getNodeDir(fetch, {
    ...opts,
    useNodeVersion: nodeVersion,
    nodeMirrorBaseUrl,
  })
  globalInfo(`Node.js ${nodeVersion as string} was installed
  ${nodeDir}`)
  return { nodeVersion, nodeDir, nodeMirrorBaseUrl }
}
