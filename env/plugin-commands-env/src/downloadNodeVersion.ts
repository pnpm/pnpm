import { globalInfo } from '@pnpm/logger'
import { createFetchFromRegistry } from '@pnpm/fetch'
import { resolveNodeVersion } from '@pnpm/node.resolver'
import type { NvmNodeCommandOptions } from '@pnpm/types'

import { getNodeDir } from './node.js'
import { getNodeMirror } from './getNodeMirror.js'
import { parseEnvSpecifier } from './parseEnvSpecifier.js'

export async function getNodeVersion(
  opts: NvmNodeCommandOptions,
  envSpecifier?: string | undefined
): Promise<{
    nodeVersion: string | null;
    nodeMirrorBaseUrl: string;
    releaseChannel?: string | undefined;
    versionSpecifier?: string | undefined;
  }> {
  const fetch = createFetchFromRegistry(opts)

  const { releaseChannel, versionSpecifier } = parseEnvSpecifier(envSpecifier)

  const nodeMirrorBaseUrl = getNodeMirror(opts.rawConfig, releaseChannel)

  const nodeVersion = await resolveNodeVersion(
    fetch,
    versionSpecifier,
    nodeMirrorBaseUrl
  )

  return { nodeVersion, nodeMirrorBaseUrl, releaseChannel, versionSpecifier }
}

export async function downloadNodeVersion(
  opts: NvmNodeCommandOptions,
  envSpecifier?: string | undefined
): Promise<{
  nodeVersion: string;
  nodeDir: string;
  nodeMirrorBaseUrl: string;
} | null> {
  const fetch = createFetchFromRegistry(opts)

  const { nodeVersion, nodeMirrorBaseUrl } = await getNodeVersion(
    opts,
    envSpecifier
  )

  if (!nodeVersion) {
    return null
  }

  const nodeDir = await getNodeDir(fetch, {
    ...opts,
    useNodeVersion: nodeVersion,
    nodeMirrorBaseUrl,
  })

  globalInfo(`Node.js ${nodeVersion as string} was installed ${nodeDir}`)

  return { nodeVersion, nodeDir, nodeMirrorBaseUrl }
}
