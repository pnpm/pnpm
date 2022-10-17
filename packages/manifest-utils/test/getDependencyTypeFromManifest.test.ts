import { getDependencyTypeFromManifest } from '@pnpm/manifest-utils'

test('getDependencyTypeFromManifest()', () => {
  expect(
    getDependencyTypeFromManifest({
      dependencies: {
        foo: '1.0.0',
      },
    }, 'foo')).toEqual('dependencies')

  expect(
    getDependencyTypeFromManifest({
      devDependencies: {
        foo: '1.0.0',
      },
    }, 'foo')).toEqual('devDependencies')

  expect(
    getDependencyTypeFromManifest({
      optionalDependencies: {
        foo: '1.0.0',
      },
    }, 'foo')).toEqual('optionalDependencies')

  expect(
    getDependencyTypeFromManifest({
      peerDependencies: {
        foo: '1.0.0',
      },
    }, 'foo')).toEqual('peerDependencies')

  expect(
    getDependencyTypeFromManifest({
      peerDependencies: {
        foo: '1.0.0',
      },
    }, 'bar')).toEqual(null)
})
