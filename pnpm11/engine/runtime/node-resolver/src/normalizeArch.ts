export function getNormalizedArch (platform: string, arch: string, nodeVersion?: string): string {
  if (nodeVersion) {
    const nodeMajorVersion = +nodeVersion.split('.')[0]
    if (arch === 'arm64' && (
      (platform === 'darwin' && nodeMajorVersion < 16) ||
      (platform === 'win32' && nodeMajorVersion < 20)
    )) {
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
