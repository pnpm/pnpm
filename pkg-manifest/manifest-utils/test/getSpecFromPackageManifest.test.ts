import { getSpecFromPackageManifest } from '@pnpm/manifest-utils'

test('getSpecFromPackageManifest()', () => {
  expect(
    getSpecFromPackageManifest({
      dependencies: {
        foo: '3.0.0',
      },
      devDependencies: {
        foo: '2.0.0',
      },
      optionalDependencies: {
        foo: '1.0.0',
      },
    }, 'foo')).toBe('1.0.0')

  expect(
    getSpecFromPackageManifest({
      dependencies: {
        foo: '3.0.0',
      },
      devDependencies: {
        foo: '2.0.0',
      },
    }, 'foo')).toBe('3.0.0')

  expect(
    getSpecFromPackageManifest({
      devDependencies: {
        foo: '2.0.0',
      },
    }, 'foo')).toBe('2.0.0')
})
