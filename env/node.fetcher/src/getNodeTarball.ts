import { getNormalizedArch } from './normalizeArch'

export interface NodeArtifactAddress {
  basename: string
  extname: string
  dirname: string
}

export function getNodeTarball (
  nodeVersion: string,
  nodeMirror: string,
  processPlatform: string,
  processArch: string
): NodeArtifactAddress {
  const platform = processPlatform === 'win32' ? 'win' : processPlatform
  const arch = getNormalizedArch(processPlatform, processArch, nodeVersion)
  const extension = platform === 'win' ? '.zip' : '.tar.gz'
  const basename = `node-v${nodeVersion}-${platform}-${arch}`
  const versionDir = `${nodeMirror}v${nodeVersion}`
  return {
    basename,
    extname: extension,
    dirname: versionDir,
  }
}
