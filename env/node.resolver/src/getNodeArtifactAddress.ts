import { getNormalizedArch } from './normalizeArch.js'

export interface NodeArtifactAddress {
  basename: string
  extname: string
  dirname: string
}

export interface GetNodeArtifactAddressOptions {
  version: string
  baseUrl: string
  platform: string
  arch: string
}

export function getNodeArtifactAddress ({
  version,
  baseUrl,
  platform,
  arch,
}: GetNodeArtifactAddressOptions): NodeArtifactAddress {
  const isWindowsPlatform = platform === 'win32'
  const normalizedPlatform = isWindowsPlatform ? 'win' : platform
  const normalizedArch = getNormalizedArch(platform, arch, version)
  return {
    dirname: `${baseUrl}v${version}`,
    basename: `node-v${version}-${normalizedPlatform}-${normalizedArch}`,
    extname: isWindowsPlatform ? '.zip' : '.tar.gz',
  }
}
