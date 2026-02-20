import { getCurrentPackageName } from '@pnpm/cli-meta'

test('getCurrentPackageName() returns pnpm when not running as SEA', () => {
  // In a test environment (not a SEA binary), getCurrentPackageName always returns 'pnpm'
  expect(getCurrentPackageName()).toBe('pnpm')
})
