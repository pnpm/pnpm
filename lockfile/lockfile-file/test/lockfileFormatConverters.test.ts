import { convertToLockfileObject } from '../lib/lockfileFormatConverters'

test('convertToLockfileObject converts directory dependencies', () => {
  expect(convertToLockfileObject({
    lockfileVersion: '',
    importers: {
      '.': {
        dependencies: {
          a: {
            specifier: 'file:../a',
            version: 'file:../a',
          },
        },
      },
    },
    packages: {
      'file:../a': {
        resolution: { directory: '../a', type: 'directory' },
        name: 'a',
        dev: false,
      } as any, // eslint-disable-line
    },
  })).toMatchObject({
    lockfileVersion: '',
    importers: {
      '.': {
        dependencies: {
          a: 'file:../a',
        },
        specifiers: {
          a: 'file:../a',
        },
      },
    },
    packages: {
      'a@file:../a': {
        resolution: { directory: '../a', type: 'directory' },
        name: 'a',
      },
    },
  })
})
