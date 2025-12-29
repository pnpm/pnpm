import { unwrapPackageName } from '../lib/unwrapPackageName.js'

test('works for wanted dependency with no alias', () => {
  expect(unwrapPackageName('is-positive', '^3.1.0')).toEqual({ pkgName: 'is-positive', bareSpecifier: '^3.1.0' })
})

test('works for wanted dependency with alias', () => {
  expect(unwrapPackageName('my-alias', 'npm:is-positive@^3.1.0')).toEqual({ pkgName: 'is-positive', bareSpecifier: '^3.1.0' })
  expect(unwrapPackageName('my-alias', 'npm:@pnpm.e2e/foo@^100.0.0')).toEqual({ pkgName: '@pnpm.e2e/foo', bareSpecifier: '^100.0.0' })
})

test('works for alias with no spec', () => {
  expect(unwrapPackageName('my-alias', 'npm:is-positive')).toEqual({ pkgName: 'is-positive', bareSpecifier: '*' })
  expect(unwrapPackageName('my-alias', 'npm:@pnpm.e2e/foo')).toEqual({ pkgName: '@pnpm.e2e/foo', bareSpecifier: '*' })
})
