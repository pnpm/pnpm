import { getCurrentPackageName } from '@pnpm/cli-meta'

test('getCurrentPackageName() returns pnpm when not running as SEA', () => {
  // In a test environment (not a SEA binary), getCurrentPackageName always returns 'pnpm'
  expect(getCurrentPackageName({
    platform: 'darwin',
    arch: 'arm64',
  })).toBe('pnpm')
  expect(getCurrentPackageName({
    platform: 'win32',
    arch: 'ia32',
  })).toBe('pnpm')
  expect(getCurrentPackageName({
    platform: 'linux',
    arch: 'x64',
  })).toBe('pnpm')
})
