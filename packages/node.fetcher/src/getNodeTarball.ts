import normalizeArch from './normalizeArch'

export function getNodeTarball (
  nodeVersion: string,
  nodeMirror: string,
  processPlatform: string,
  processArch: string
) {
  const platform = processPlatform === 'win32' ? 'win' : processPlatform
  const arch = normalizeArch(processPlatform, processArch)
  const nodeMajorVersion = +nodeVersion.split('.')[0]
  const nodeBinaryArch = (platform === 'darwin' && arch === 'arm64' && (nodeMajorVersion < 16)) ? 'x64' : arch
  const extension = platform === 'win' ? 'zip' : 'tar.gz'
  const pkgName = `node-v${nodeVersion}-${platform}-${nodeBinaryArch}`
  return {
    pkgName,
    tarball: `${nodeMirror}v${nodeVersion}/${pkgName}.${extension}`,
  }
}
