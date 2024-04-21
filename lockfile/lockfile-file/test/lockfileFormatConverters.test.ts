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

test('convertToLockfileObject converts git-hosted dependencies', () => {
  expect(convertToLockfileObject({
    lockfileVersion: '',
    importers: {
      '.': {
        dependencies: {
          'is-negative': {
            specifier: 'github:kevva/is-negative',
            version: 'github.com/kevva/is-negative/1d7e288222b53a0cab90a331f1865220ec29560c',
          },
        },
      },
    },
    packages: {
      'github.com/kevva/is-negative/1d7e288222b53a0cab90a331f1865220ec29560c': {
        resolution: { tarball: 'https://codeload.github.com/kevva/is-negative/tar.gz/1d7e288222b53a0cab90a331f1865220ec29560c' },
        name: 'is-negative',
        version: '2.1.0',
        engines: { node: '>=0.10.0' },
        dev: false,
      } as any, // eslint-disable-line
    },
  })).toMatchObject({
    lockfileVersion: '',
    importers: {
      '.': {
        dependencies: {
          'is-negative': 'https://codeload.github.com/kevva/is-negative/tar.gz/1d7e288222b53a0cab90a331f1865220ec29560c',
        },
        specifiers: {
          'is-negative': 'github:kevva/is-negative',
        },
      },
    },
    packages: {
      'is-negative@https://codeload.github.com/kevva/is-negative/tar.gz/1d7e288222b53a0cab90a331f1865220ec29560c': {
        resolution: { tarball: 'https://codeload.github.com/kevva/is-negative/tar.gz/1d7e288222b53a0cab90a331f1865220ec29560c' },
        name: 'is-negative',
        version: '2.1.0',
        engines: { node: '>=0.10.0' },
      },
    },
  })
})
