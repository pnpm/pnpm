import { getNormalizedArch } from './normalizeArch'

export function getNodeTarball (
  nodeVersion: string,
  nodeMirror: string,
  processPlatform: string,
  processArch: string
): { pkgName: string, tarball: string } {
  const platform = processPlatform === 'win32' ? 'win' : processPlatform
  const arch = getNormalizedArch(processPlatform, processArch, nodeVersion)
  const extension = platform === 'win' ? 'zip' : 'tar.gz'
  const pkgName = `node-v${nodeVersion}-${platform}-${arch}`
  return {
    pkgName,
    tarball: `${nodeMirror}v${nodeVersion}/${pkgName}.${extension}`,
  }
}
