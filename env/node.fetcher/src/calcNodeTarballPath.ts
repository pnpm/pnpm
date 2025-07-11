import { getNormalizedArch } from './normalizeArch'

export interface NodeArtifactAddress {
  basename: string
  extname: string
  dirname: string
}

export function calcNodeTarballPath ({
  version,
  baseUrl,
  platform,
  arch,
}: {
  version: string
  baseUrl: string
  platform: string
  arch: string
}): NodeArtifactAddress {
  const isWindowsPlatform = platform === 'win32'
  const normalizedPlatform = isWindowsPlatform ? 'win' : platform
  const normalizedArch = getNormalizedArch(platform, arch, version)
  return {
    basename: `node-v${version}-${normalizedPlatform}-${normalizedArch}`,
    extname: isWindowsPlatform ? '.zip' : '.tar.gz',
    dirname: `${baseUrl}v${version}`,
  }
}
