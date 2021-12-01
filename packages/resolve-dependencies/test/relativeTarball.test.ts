/// <reference path="../../../typings/index.d.ts" />
import { relativeTarball } from '@pnpm/resolve-dependencies/lib/updateLockfile'

test('relativeTarball()', () => {
  expect(relativeTarball('https://registry.com/foo/bar.tgz', 'https://registry.com/foo')).toBe('bar.tgz')
  expect(relativeTarball('https://registry.com/foo/bar.tgz', 'https://registry.com/foo/')).toBe('bar.tgz')
})
