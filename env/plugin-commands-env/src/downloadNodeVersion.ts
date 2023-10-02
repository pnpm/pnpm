import { resolveNodeVersion } from '@pnpm/node.resolver'
import { getNodeMirror } from './getNodeMirror'
import { getNodeDir, type NvmNodeCommandOptions } from './node'
import { parseEnvSpecifier } from './parseEnvSpecifier'
import { createFetchFromRegistry } from '@pnpm/fetch'
import { PnpmError } from '@pnpm/error'
import { globalInfo } from '@pnpm/logger'

export async function getNodeVersion (opts: NvmNodeCommandOptions, version: string) {
  const fetch = createFetchFromRegistry(opts)
  const { releaseChannel, versionSpecifier } = parseEnvSpecifier(version)
  const nodeMirrorBaseUrl = getNodeMirror(opts.rawConfig, releaseChannel)
  const nodeVersion = await resolveNodeVersion(fetch, versionSpecifier, nodeMirrorBaseUrl)
  return { nodeVersion, nodeMirrorBaseUrl, releaseChannel, versionSpecifier }
}

export async function downloadNodeVersion (opts: NvmNodeCommandOptions, version: string) {
  const fetch = createFetchFromRegistry(opts)
  const { nodeVersion, nodeMirrorBaseUrl } = await getNodeVersion(opts, version)
  if (!nodeVersion) {
    return new PnpmError('COULD_NOT_RESOLVE_NODEJS', `Couldn't find Node.js version matching ${version}`)
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
