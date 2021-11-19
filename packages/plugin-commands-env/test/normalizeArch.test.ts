import normalizeArch from '@pnpm/plugin-commands-env/lib/normalizeArch'

test.each([
  ['win32', 'ia32', 'x86'],
  ['linux', 'arm', 'armv7l'], // Raspberry Pi 4
  ['linux', 'x64', 'x64'],
])('%s is normalized to %s', (platform, arch, normalizedArch) => {
  expect(normalizeArch(platform, arch)).toBe(normalizedArch)
})
