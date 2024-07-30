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

test('convertToLockfileObject converts git-hosted dependencies via ssh', () => {
  const result = convertToLockfileObject({
    lockfileVersion: '',
    importers: {
      '.': {
        dependencies: {
          'git-resolver': {
            specifier: 'ssh://git@gitlab:pnpm/git-resolver#988c61e11dc8d9ca0b5580cb15291951812549dc',
            version: 'git+ssh://git@gitlab/pnpm/git-resolver#988c61e11dc8d9ca0b5580cb15291951812549dc',
          },
          foo: {
            specifier: 'https://gitlab.com/foo/foo.git',
            version: 'git@gitlab.com+foo/foo/6ae3f32d7c631f64fbaf70cdd349ae6e2cc68e6c',
          },
        },
      },
    },
    packages: {
      'git+ssh://git@gitlab/pnpm/git-resolver#988c61e11dc8d9ca0b5580cb15291951812549dc': {
        name: 'git-resolver',
        resolution: {
          commit: '988c61e11dc8d9ca0b5580cb15291951812549dc',
          repo: 'ssh://git@gitlab/pnpm/git-resolver',
          type: 'git',
        },
      } as any, // eslint-disable-line
      'git@gitlab.com+foo/foo/6ae3f32d7c631f64fbaf70cdd349ae6e2cc68e6c': {
        name: 'foo',
        resolution: {
          commit: '6ae3f32d7c631f64fbaf70cdd349ae6e2cc68e6c',
          repo: 'git@gitlab.com:foo/foo.git',
          type: 'git',
        },
      } as any, // eslint-disable-line
    },
  })
  expect(result).toMatchObject({
    lockfileVersion: '',
    importers: {
      '.': {
        dependencies: {
          foo: 'git+https://git@gitlab.com:foo/foo.git#6ae3f32d7c631f64fbaf70cdd349ae6e2cc68e6c',
          'git-resolver': 'git+ssh://git@gitlab/pnpm/git-resolver#988c61e11dc8d9ca0b5580cb15291951812549dc',
        },
        specifiers: {
          foo: 'https://gitlab.com/foo/foo.git',
          'git-resolver': 'ssh://git@gitlab:pnpm/git-resolver#988c61e11dc8d9ca0b5580cb15291951812549dc',
        },
      },
    },
    packages: {
      'git-resolver@git+ssh://git@gitlab/pnpm/git-resolver#988c61e11dc8d9ca0b5580cb15291951812549dc': {
        resolution: {
          commit: '988c61e11dc8d9ca0b5580cb15291951812549dc',
          repo: 'ssh://git@gitlab/pnpm/git-resolver',
          type: 'git',
        },
      },
      'foo@git+https://git@gitlab.com:foo/foo.git#6ae3f32d7c631f64fbaf70cdd349ae6e2cc68e6c': {
        resolution: {
          commit: '6ae3f32d7c631f64fbaf70cdd349ae6e2cc68e6c',
          repo: 'git@gitlab.com:foo/foo.git',
          type: 'git',
        },
      },
    },
  })
})
