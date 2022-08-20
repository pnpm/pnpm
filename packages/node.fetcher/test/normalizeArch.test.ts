import normalizeArch from '../lib/normalizeArch'

test.each([
  ['win32', 'ia32', 'x86'],
  ['linux', 'arm', 'armv7l'], // Raspberry Pi 4
  ['linux', 'x64', 'x64'],
])('normalizedArch(%s, %s)', (platform, arch, normalizedArch) => {
  expect(normalizeArch(platform, arch)).toBe(normalizedArch)
})

// macos apple silicon
test.each([
  ['darwin', 'arm64', '14.20.0', 'x64'],
  ['darwin', 'arm64', '16.17.0', 'arm64'],
])('normalizedArch(%s, %s)', (platform, arch, nodeVersion, normalizedArch) => {
  expect(normalizeArch(platform, arch, nodeVersion)).toBe(normalizedArch)
})
