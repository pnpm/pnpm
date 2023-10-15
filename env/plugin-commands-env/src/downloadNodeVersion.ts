import { resolveNodeVersion } from '@pnpm/node.resolver'
import { getNodeMirror } from './getNodeMirror'
import { getNodeDir, type NvmNodeCommandOptions } from './node'
import { parseEnvSpecifier } from './parseEnvSpecifier'
import { createFetchFromRegistry } from '@pnpm/fetch'
import { globalInfo } from '@pnpm/logger'

export async function getNodeVersion (opts: NvmNodeCommandOptions, envSpecifier: string) {
  const fetch = createFetchFromRegistry(opts)
  const { releaseChannel, versionSpecifier } = parseEnvSpecifier(envSpecifier)
  const nodeMirrorBaseUrl = getNodeMirror(opts.rawConfig, releaseChannel)
  const nodeVersion = await resolveNodeVersion(fetch, versionSpecifier, nodeMirrorBaseUrl)
  return { nodeVersion, nodeMirrorBaseUrl, releaseChannel, versionSpecifier }
}

export async function downloadNodeVersion (opts: NvmNodeCommandOptions, envSpecifier: string) {
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
