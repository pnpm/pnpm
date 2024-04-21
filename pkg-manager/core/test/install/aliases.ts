import { LOCKFILE_VERSION } from '@pnpm/constants'
import { prepareEmpty } from '@pnpm/prepare'
import { addDistTag, getIntegrity } from '@pnpm/registry-mock'
import { addDependenciesToPackage } from '@pnpm/core'
import { testDefaults } from '../utils'

test('installing aliased dependency', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['negative@npm:is-negative@1.0.0', 'positive@npm:is-positive'], testDefaults({ fastUnpack: false }))

  const m = project.requireModule('negative')
  expect(typeof m).toBe('function')
  expect(typeof project.requireModule('positive')).toBe('function')

  expect(project.readLockfile()).toStrictEqual({
    settings: {
      autoInstallPeers: true,
      excludeLinksFromLockfile: false,
    },
    importers: {
      '.': {
        dependencies: {
          negative: {
            specifier: 'npm:is-negative@1.0.0',
            version: 'is-negative@1.0.0',
          },
          positive: {
            specifier: 'npm:is-positive@^3.1.0',
            version: 'is-positive@3.1.0',
          },
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      'is-negative@1.0.0': {
        engines: {
          node: '>=0.10.0',
        },
        resolution: {
          integrity: 'sha512-1aKMsFUc7vYQGzt//8zhkjRWPoYkajY/I5MJEvrc0pDoHXrW7n5ri8DYxhy3rR+Dk0QFl7GjHHsZU1sppQrWtw==',
        },
      },
      'is-positive@3.1.0': {
        engines: {
          node: '>=0.10.0',
        },
        resolution: {
          integrity: 'sha512-8ND1j3y9/HP94TOvGzr69/FgbkX2ruOldhLEsTWwcJVfo4oRjwemJmJxt7RJkKYH8tz7vYBP9JcKQY8CLuJ90Q==',
        },
      },
    },
    snapshots: {
      'is-negative@1.0.0': {},
      'is-positive@3.1.0': {
      },
    },
  })
})

test('aliased dependency w/o version spec, with custom tag config', async () => {
  const project = prepareEmpty()

  const tag = 'beta'

  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.0.0', distTag: tag })

  await addDependenciesToPackage({}, ['foo@npm:@pnpm.e2e/dep-of-pkg-with-1-dep'], testDefaults({ tag }))

  project.storeHas('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.0.0')
})

test('a dependency has an aliased subdependency', async () => {
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-1-aliased-dep'], testDefaults({ fastUnpack: false }))

  expect(project.requireModule('@pnpm.e2e/pkg-with-1-aliased-dep')().name).toEqual('@pnpm.e2e/dep-of-pkg-with-1-dep')

  expect(project.readLockfile()).toStrictEqual({
    settings: {
      autoInstallPeers: true,
      excludeLinksFromLockfile: false,
    },
    importers: {
      '.': {
        dependencies: {
          '@pnpm.e2e/pkg-with-1-aliased-dep': {
            specifier: '^100.0.0',
            version: '100.0.0',
          },
        },
      },
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0': {
        resolution: {
          integrity: getIntegrity('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.1.0'),
        },
      },
      '@pnpm.e2e/pkg-with-1-aliased-dep@100.0.0': {
        resolution: {
          integrity: getIntegrity('@pnpm.e2e/pkg-with-1-aliased-dep', '100.0.0'),
        },
      },
    },
    snapshots: {
      '@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0': {},
      '@pnpm.e2e/pkg-with-1-aliased-dep@100.0.0': {
        dependencies: {
          dep: '@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0',
        },
      },
    },
  })
})

test('installing the same package via an alias and directly', async () => {
  const project = prepareEmpty()
  const manifest = await addDependenciesToPackage({}, ['negative@npm:is-negative@^1.0.1', 'is-negative@^1.0.1'], testDefaults({ fastUnpack: false }))

  expect(manifest.dependencies).toStrictEqual({ negative: 'npm:is-negative@^1.0.1', 'is-negative': '^1.0.1' })

  expect(typeof project.requireModule('negative')).toEqual('function')
  expect(typeof project.requireModule('is-negative')).toEqual('function')
})
