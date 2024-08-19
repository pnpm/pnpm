import { getCurrentPackageName } from '@pnpm/cli-meta'

test('getCurrentPackageName()', () => {
  expect(getCurrentPackageName({
    platform: 'darwin',
    arch: 'arm64',
  })).toBe('pnpm')
  expect(getCurrentPackageName({
    pkg: '.',
    platform: 'win32',
    arch: 'ia32',
  })).toBe('@pnpm/win-x86')
  expect(getCurrentPackageName({
    pkg: '.',
    platform: 'darwin',
    arch: 'arm64',
  })).toBe('@pnpm/macos-arm64')
  expect(getCurrentPackageName({
    pkg: '.',
    platform: 'linux',
    arch: 'x64',
  })).toBe('@pnpm/linux-x64')
})
