/* eslint-disable no-await-in-loop */
import { PnpmError } from '@pnpm/error'
import { createFetchFromRegistry } from '@pnpm/fetch'
import { resolveNodeVersion } from '@pnpm/node.resolver'
import { getNodeMirror } from './getNodeMirror'
import { type NvmNodeCommandOptions, getNodeDir } from './node'
import { parseEnvSpecifier } from './parseEnvSpecifier'
import { globalInfo } from '@pnpm/logger'

export async function envAdd (opts: NvmNodeCommandOptions, params: string[]) {
  if (!opts.global) {
    throw new PnpmError('NOT_IMPLEMENTED_YET', '"pnpm env use <version>" can only be used with the "--global" option currently')
  }
  const fetch = createFetchFromRegistry(opts)
  for (const version of params) {
    const { releaseChannel, versionSpecifier } = parseEnvSpecifier(version)
    const nodeMirrorBaseUrl = getNodeMirror(opts.rawConfig, releaseChannel)
    const nodeVersion = await resolveNodeVersion(fetch, versionSpecifier, nodeMirrorBaseUrl)
    if (!nodeVersion) {
      throw new PnpmError('COULD_NOT_RESOLVE_NODEJS', `Couldn't find Node.js version matching ${version}`)
    }
    const nodeDir = await getNodeDir(fetch, {
      ...opts,
      useNodeVersion: nodeVersion,
      nodeMirrorBaseUrl,
    })
    globalInfo(`Node.js ${nodeVersion as string} was installed
  ${nodeDir}`)
  }
  return 'All specified Node.js versions were installed'
}
