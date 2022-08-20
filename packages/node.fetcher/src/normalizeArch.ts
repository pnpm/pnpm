export default function getNormalizedArch (platform: string, arch: string, nodeVersion?: string) {
  if (nodeVersion) {
    const nodeMajorVersion = +nodeVersion.split('.')[0]
    if ((platform === 'darwin' && arch === 'arm64' && (nodeMajorVersion < 16))) {
      return 'x64'
    }
  }
  if (platform === 'win32' && arch === 'ia32') {
    return 'x86'
  }
  if (arch === 'arm') {
    return 'armv7l'
  }
  return arch
}
