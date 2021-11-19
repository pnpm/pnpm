export default function getNormalizedArch (platform: string, arch: string) {
  if (platform === 'win32' && arch === 'ia32') {
    return 'x86'
  }
  if (arch === 'arm') {
    return 'armv7l'
  }
  return arch
}
