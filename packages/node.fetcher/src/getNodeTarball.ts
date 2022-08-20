import normalizeArch from './normalizeArch'

export function getNodeTarball (
  nodeVersion: string,
  nodeMirror: string,
  processPlatform: string,
  processArch: string
) {
  const platform = processPlatform === 'win32' ? 'win' : processPlatform
  const arch = normalizeArch(processPlatform, processArch, nodeVersion)
  const extension = platform === 'win' ? 'zip' : 'tar.gz'
  const pkgName = `node-v${nodeVersion}-${platform}-${arch}`
  return {
    pkgName,
    tarball: `${nodeMirror}v${nodeVersion}/${pkgName}.${extension}`,
  }
}
