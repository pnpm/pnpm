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
  libc?: string
}

export function getNodeArtifactAddress ({
  version,
  baseUrl,
  platform,
  arch,
  libc,
}: GetNodeArtifactAddressOptions): NodeArtifactAddress {
  const isWindowsPlatform = platform === 'win32'
  const normalizedPlatform = isWindowsPlatform ? 'win' : platform
  const normalizedArch = getNormalizedArch(platform, arch, version)
  const archSuffix = libc === 'musl' ? '-musl' : ''
  return {
    dirname: `${baseUrl}v${version}`,
    basename: `node-v${version}-${normalizedPlatform}-${normalizedArch}${archSuffix}`,
    extname: isWindowsPlatform ? '.zip' : '.tar.gz',
  }
}
