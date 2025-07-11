import { getNormalizedArch } from './normalizeArch'

export interface NodeArtifactAddress {
  basename: string
  extname: string
  dirname: string
}

export interface CalcNodeTarballPathOptions {
  version: string
  baseUrl: string
  platform: string
  arch: string
}

export function calcNodeTarballPath ({
  version,
  baseUrl,
  platform,
  arch,
}: CalcNodeTarballPathOptions): NodeArtifactAddress {
  const isWindowsPlatform = platform === 'win32'
  const normalizedPlatform = isWindowsPlatform ? 'win' : platform
  const normalizedArch = getNormalizedArch(platform, arch, version)
  return {
    dirname: `${baseUrl}v${version}`,
    basename: `node-v${version}-${normalizedPlatform}-${normalizedArch}`,
    extname: isWindowsPlatform ? '.zip' : '.tar.gz',
  }
}
