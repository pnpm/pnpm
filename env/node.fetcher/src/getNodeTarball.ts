import { getNormalizedArch } from './normalizeArch'

export function getNodeTarball (
  nodeVersion: string,
  nodeMirror: string,
  processPlatform: string,
  processArch: string
): { pkgName: string, tarball: string, integritiesFileUrl: string, fileName: string } {
  const platform = processPlatform === 'win32' ? 'win' : processPlatform
  const arch = getNormalizedArch(processPlatform, processArch, nodeVersion)
  const extension = platform === 'win' ? 'zip' : 'tar.gz'
  const pkgName = `node-v${nodeVersion}-${platform}-${arch}`
  const versionDir = `${nodeMirror}v${nodeVersion}`
  const fileName = `${pkgName}.${extension}`
  return {
    pkgName,
    fileName,
    tarball: `${versionDir}/${fileName}`,
    integritiesFileUrl: `${versionDir}/SHASUMS256.txt`,
  }
}
