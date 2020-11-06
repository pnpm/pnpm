import { Lockfile } from '@pnpm/lockfile-types'
import mergeLockfileChanges from '../src'

const simpleLockfile = {
  importers: {
    '.': {
      dependencies: {
        foo: '1.0.0',
      },
      specifiers: {
        foo: '1.0.0',
      },
    },
  },
  lockfileVersion: 5.2,
  packages: {
    '/foo/1.0.0': {
      resolution: {
        integrity: 'sha512-aBVzCAzfyApU0gg36QgCpJixGtYwuQ4djrn11J+DTB5vE4OmBPuZiulgTCA9ByULgVAyNV2CTpjjvZmxzukSLw==',
      },
    },
  },
}

test('fails when specifiers differ', () => {
  expect(() => {
    mergeLockfileChanges({
      base: simpleLockfile,
      ours: {
        ...simpleLockfile,
        importers: {
          '.': {
            ...simpleLockfile.importers['.'],
            specifiers: { foo: '^1.0.0' },
          },
        },
      },
      theirs: {
        ...simpleLockfile,
        importers: {
          '.': {
            ...simpleLockfile.importers['.'],
            specifiers: { foo: '^1.1.0' },
          },
        },
      },
    })
  }).toThrowError(/Cannot resolve 'specifiers.foo'/)
})

test('picks the newer version when dependencies differ inside importer', () => {
  const mergedLockfile = mergeLockfileChanges({
    base: simpleLockfile,
    ours: {
      ...simpleLockfile,
      importers: {
        '.': {
          ...simpleLockfile.importers['.'],
          dependencies: {
            foo: '1.2.0',
            bar: '3.0.0_qar@1.0.0',
            zoo: '4.0.0_qar@1.0.0',
          },
        },
      },
    },
    theirs: {
      ...simpleLockfile,
      importers: {
        '.': {
          ...simpleLockfile.importers['.'],
          dependencies: {
            foo: '1.1.0',
            bar: '4.0.0_qar@1.0.0',
            zoo: '3.0.0_qar@1.0.0',
          },
        },
      },
    },
  })
  expect(mergedLockfile.importers['.'].dependencies?.foo).toBe('1.2.0')
  expect(mergedLockfile.importers['.'].dependencies?.bar).toBe('4.0.0_qar@1.0.0')
  expect(mergedLockfile.importers['.'].dependencies?.zoo).toBe('4.0.0_qar@1.0.0')
})

test('picks the newer version when dependencies differ inside package', () => {
  const base: Lockfile = {
    importers: {
      '.': {
        dependencies: {
          a: '1.0.0',
        },
        specifiers: {},
      },
    },
    lockfileVersion: 5.2,
    packages: {
      '/a/1.0.0': {
        dependencies: {
          foo: '1.0.0',
        },
        resolution: {
          integrity: '',
        },
      },
      '/foo/1.0.0': {
        resolution: {
          integrity: '',
        },
      },
    },
  }
  const mergedLockfile = mergeLockfileChanges({
    base,
    ours: {
      ...base,
      packages: {
        ...base.packages,
        '/a/1.0.0': {
          dependencies: {
            linked: 'link:../1',
            foo: '1.2.0',
            bar: '3.0.0_qar@1.0.0',
            zoo: '4.0.0_qar@1.0.0',
            qar: '1.0.0',
          },
          resolution: {
            integrity: '',
          },
        },
        '/bar/3.0.0_qar@1.0.0': {
          dependencies: {
            qar: '1.0.0',
          },
          resolution: {
            integrity: '',
          },
        },
        '/zoo/4.0.0_qar@1.0.0': {
          dependencies: {
            qar: '1.0.0',
          },
          resolution: {
            integrity: '',
          },
        },
        '/foo/1.2.0': {
          resolution: {
            integrity: '',
          },
        },
        '/qar/1.0.0': {
          resolution: {
            integrity: '',
          },
        },
      },
    },
    theirs: {
      ...base,
      packages: {
        ...base.packages,
        '/a/1.0.0': {
          dependencies: {
            linked: 'link:../1',
            foo: '1.1.0',
            bar: '4.0.0_qar@1.0.0',
            zoo: '3.0.0_qar@1.0.0',
            qar: '1.0.0',
          },
          resolution: {
            integrity: '',
          },
        },
        '/bar/4.0.0_qar@1.0.0': {
          dependencies: {
            qar: '1.0.0',
          },
          resolution: {
            integrity: '',
          },
        },
        '/zoo/3.0.0_qar@1.0.0': {
          dependencies: {
            qar: '1.0.0',
          },
          resolution: {
            integrity: '',
          },
        },
        '/foo/1.1.0': {
          resolution: {
            integrity: '',
          },
        },
        '/qar/1.0.0': {
          resolution: {
            integrity: '',
          },
        },
      },
    },
  })
  expect(mergedLockfile.packages?.['/a/1.0.0'].dependencies?.linked).toBe('link:../1')
  expect(mergedLockfile.packages?.['/a/1.0.0'].dependencies?.foo).toBe('1.2.0')
  expect(mergedLockfile.packages?.['/a/1.0.0'].dependencies?.bar).toBe('4.0.0_qar@1.0.0')
  expect(mergedLockfile.packages?.['/a/1.0.0'].dependencies?.zoo).toBe('4.0.0_qar@1.0.0')
  expect(Object.keys(mergedLockfile.packages ?? {}).sort()).toStrictEqual([
    '/a/1.0.0',
    '/bar/3.0.0_qar@1.0.0',
    '/bar/4.0.0_qar@1.0.0',
    '/foo/1.0.0',
    '/foo/1.1.0',
    '/foo/1.2.0',
    '/qar/1.0.0',
    '/zoo/3.0.0_qar@1.0.0',
    '/zoo/4.0.0_qar@1.0.0',
  ])
})

test('prefers our lockfile resolutions when it has newer packages', () => {
  const mergedLockfile = mergeLockfileChanges({
    base: simpleLockfile,
    ours: {
      ...simpleLockfile,
      packages: {
        '/foo/1.0.0': {
          dependencies: {
            bar: '1.0.0',
          },
          resolution: {
            integrity: 'sha512-aBVzCAzfyApU0gg36QgCpJixGtYwuQ4djrn11J+DTB5vE4OmBPuZiulgTCA9ByULgVAyNV2CTpjjvZmxzukSLw==',
          },
        },
        '/bar/1.0.0': {
          resolution: {
            integrity: 'sha512-aBVzCAzfyApU0gg36QgCpJixGtYwuQ4djrn11J+DTB5vE4OmBPuZiulgTCA9ByULgVAyNV2CTpjjvZmxzukSLw==',
          },
        },
      },
    },
    theirs: {
      ...simpleLockfile,
      packages: {
        '/foo/1.0.0': {
          dependencies: {
            bar: '1.1.0',
          },
          resolution: {
            integrity: 'sha512-aBVzCAzfyApU0gg36QgCpJixGtYwuQ4djrn11J+DTB5vE4OmBPuZiulgTCA9ByULgVAyNV2CTpjjvZmxzukSLw==',
          },
        },
        '/bar/1.1.0': {
          dependencies: {
            qar: '1.0.0',
          },
          resolution: {
            integrity: 'sha512-aBVzCAzfyApU0gg36QgCpJixGtYwuQ4djrn11J+DTB5vE4OmBPuZiulgTCA9ByULgVAyNV2CTpjjvZmxzukSLw==',
          },
        },
        '/qar/1.0.0': {
          resolution: {
            integrity: 'sha512-aBVzCAzfyApU0gg36QgCpJixGtYwuQ4djrn11J+DTB5vE4OmBPuZiulgTCA9ByULgVAyNV2CTpjjvZmxzukSLw==',
          },
        },
      },
    },
  })

  expect(mergedLockfile).toStrictEqual({
    ...simpleLockfile,
    packages: {
      '/foo/1.0.0': {
        dependencies: {
          bar: '1.1.0',
        },
        resolution: {
          integrity: 'sha512-aBVzCAzfyApU0gg36QgCpJixGtYwuQ4djrn11J+DTB5vE4OmBPuZiulgTCA9ByULgVAyNV2CTpjjvZmxzukSLw==',
        },
      },
      '/bar/1.0.0': {
        resolution: {
          integrity: 'sha512-aBVzCAzfyApU0gg36QgCpJixGtYwuQ4djrn11J+DTB5vE4OmBPuZiulgTCA9ByULgVAyNV2CTpjjvZmxzukSLw==',
        },
      },
      '/bar/1.1.0': {
        dependencies: {
          qar: '1.0.0',
        },
        resolution: {
          integrity: 'sha512-aBVzCAzfyApU0gg36QgCpJixGtYwuQ4djrn11J+DTB5vE4OmBPuZiulgTCA9ByULgVAyNV2CTpjjvZmxzukSLw==',
        },
      },
      '/qar/1.0.0': {
        resolution: {
          integrity: 'sha512-aBVzCAzfyApU0gg36QgCpJixGtYwuQ4djrn11J+DTB5vE4OmBPuZiulgTCA9ByULgVAyNV2CTpjjvZmxzukSLw==',
        },
      },
    },
  })
})

test('prefers our lockfile resolutions when it has newer packages', () => {
  const mergedLockfile = mergeLockfileChanges({
    base: simpleLockfile,
    theirs: {
      ...simpleLockfile,
      packages: {
        '/foo/1.0.0': {
          dependencies: {
            bar: '1.0.0',
          },
          resolution: {
            integrity: 'sha512-aBVzCAzfyApU0gg36QgCpJixGtYwuQ4djrn11J+DTB5vE4OmBPuZiulgTCA9ByULgVAyNV2CTpjjvZmxzukSLw==',
          },
        },
        '/bar/1.0.0': {
          resolution: {
            integrity: 'sha512-aBVzCAzfyApU0gg36QgCpJixGtYwuQ4djrn11J+DTB5vE4OmBPuZiulgTCA9ByULgVAyNV2CTpjjvZmxzukSLw==',
          },
        },
      },
    },
    ours: {
      ...simpleLockfile,
      packages: {
        '/foo/1.0.0': {
          dependencies: {
            bar: '1.1.0',
          },
          resolution: {
            integrity: 'sha512-aBVzCAzfyApU0gg36QgCpJixGtYwuQ4djrn11J+DTB5vE4OmBPuZiulgTCA9ByULgVAyNV2CTpjjvZmxzukSLw==',
          },
        },
        '/bar/1.1.0': {
          dependencies: {
            qar: '1.0.0',
          },
          resolution: {
            integrity: 'sha512-aBVzCAzfyApU0gg36QgCpJixGtYwuQ4djrn11J+DTB5vE4OmBPuZiulgTCA9ByULgVAyNV2CTpjjvZmxzukSLw==',
          },
        },
        '/qar/1.0.0': {
          resolution: {
            integrity: 'sha512-aBVzCAzfyApU0gg36QgCpJixGtYwuQ4djrn11J+DTB5vE4OmBPuZiulgTCA9ByULgVAyNV2CTpjjvZmxzukSLw==',
          },
        },
      },
    },
  })

  expect(mergedLockfile).toStrictEqual({
    ...simpleLockfile,
    packages: {
      '/foo/1.0.0': {
        dependencies: {
          bar: '1.1.0',
        },
        resolution: {
          integrity: 'sha512-aBVzCAzfyApU0gg36QgCpJixGtYwuQ4djrn11J+DTB5vE4OmBPuZiulgTCA9ByULgVAyNV2CTpjjvZmxzukSLw==',
        },
      },
      '/bar/1.0.0': {
        resolution: {
          integrity: 'sha512-aBVzCAzfyApU0gg36QgCpJixGtYwuQ4djrn11J+DTB5vE4OmBPuZiulgTCA9ByULgVAyNV2CTpjjvZmxzukSLw==',
        },
      },
      '/bar/1.1.0': {
        dependencies: {
          qar: '1.0.0',
        },
        resolution: {
          integrity: 'sha512-aBVzCAzfyApU0gg36QgCpJixGtYwuQ4djrn11J+DTB5vE4OmBPuZiulgTCA9ByULgVAyNV2CTpjjvZmxzukSLw==',
        },
      },
      '/qar/1.0.0': {
        resolution: {
          integrity: 'sha512-aBVzCAzfyApU0gg36QgCpJixGtYwuQ4djrn11J+DTB5vE4OmBPuZiulgTCA9ByULgVAyNV2CTpjjvZmxzukSLw==',
        },
      },
    },
  })
})