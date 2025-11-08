import { getDependencyTypeFromManifest } from '@pnpm/manifest-utils'

test('getDependencyTypeFromManifest()', () => {
  expect(
    getDependencyTypeFromManifest({
      dependencies: {
        foo: '1.0.0',
      },
    }, 'foo')).toBe('dependencies')

  expect(
    getDependencyTypeFromManifest({
      devDependencies: {
        foo: '1.0.0',
      },
    }, 'foo')).toBe('devDependencies')

  expect(
    getDependencyTypeFromManifest({
      optionalDependencies: {
        foo: '1.0.0',
      },
    }, 'foo')).toBe('optionalDependencies')

  expect(
    getDependencyTypeFromManifest({
      peerDependencies: {
        foo: '1.0.0',
      },
    }, 'foo')).toBe('peerDependencies')

  expect(
    getDependencyTypeFromManifest({
      peerDependencies: {
        foo: '1.0.0',
      },
    }, 'bar')).toBeNull()
})
