import { getCurrentPackageName } from '@pnpm/cli-meta'

test('getCurrentPackageName()', () => {
  expect(getCurrentPackageName({} as unknown as NodeJS.Process)).toBe('pnpm')
  expect(getCurrentPackageName({
    pkg: '.',
    platform: 'win32',
    arch: 'ia32',
  } as unknown as NodeJS.Process)).toBe('@pnpm/win-x86')
  expect(getCurrentPackageName({
    pkg: '.',
    platform: 'darwin',
    arch: 'arm64',
  } as unknown as NodeJS.Process)).toBe('@pnpm/macos-arm64')
  expect(getCurrentPackageName({
    pkg: '.',
    platform: 'linux',
    arch: 'x64',
  } as unknown as NodeJS.Process)).toBe('@pnpm/linux-x64')
})
