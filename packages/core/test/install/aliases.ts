import { LOCKFILE_VERSION } from '@pnpm/constants'
import { prepareEmpty } from '@pnpm/prepare'
import { addDistTag, getIntegrity } from '@pnpm/registry-mock'
import { addDependenciesToPackage } from '@pnpm/core'
import { testDefaults } from '../utils'

test('installing aliased dependency', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['negative@npm:is-negative@1.0.0', 'positive@npm:is-positive'], await testDefaults({ fastUnpack: false }))

  const m = project.requireModule('negative')
  expect(typeof m).toBe('function')
  expect(typeof project.requireModule('positive')).toBe('function')

  expect(await project.readLockfile()).toStrictEqual({
    dependencies: {
      negative: '/is-negative/1.0.0',
      positive: '/is-positive/3.1.0',
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/is-negative/1.0.0': {
        dev: false,
        engines: {
          node: '>=0.10.0',
        },
        resolution: {
          integrity: 'sha1-clmHeoPIAKwxkd17nZ+80PdS1P4=',
        },
      },
      '/is-positive/3.1.0': {
        dev: false,
        engines: {
          node: '>=0.10.0',
        },
        resolution: {
          integrity: 'sha1-hX21hKG6XRyymAUn/DtsQ103sP0=',
        },
      },
    },
    specifiers: {
      negative: 'npm:is-negative@1.0.0',
      positive: 'npm:is-positive@^3.1.0',
    },
  })
})

test('aliased dependency w/o version spec, with custom tag config', async () => {
  const project = prepareEmpty()

  const tag = 'beta'

  await addDistTag({ package: 'dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })
  await addDistTag({ package: 'dep-of-pkg-with-1-dep', version: '100.0.0', distTag: tag })

  await addDependenciesToPackage({}, ['foo@npm:dep-of-pkg-with-1-dep'], await testDefaults({ tag }))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')
})

test('a dependency has an aliased subdependency', async () => {
  await addDistTag({ package: 'dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['pkg-with-1-aliased-dep'], await testDefaults({ fastUnpack: false }))

  expect(project.requireModule('pkg-with-1-aliased-dep')().name).toEqual('dep-of-pkg-with-1-dep')

  expect(await project.readLockfile()).toStrictEqual({
    dependencies: {
      'pkg-with-1-aliased-dep': '100.0.0',
    },
    lockfileVersion: LOCKFILE_VERSION,
    packages: {
      '/dep-of-pkg-with-1-dep/100.1.0': {
        dev: false,
        resolution: {
          integrity: getIntegrity('dep-of-pkg-with-1-dep', '100.1.0'),
        },
      },
      '/pkg-with-1-aliased-dep/100.0.0': {
        dependencies: {
          dep: '/dep-of-pkg-with-1-dep/100.1.0',
        },
        dev: false,
        resolution: {
          integrity: getIntegrity('pkg-with-1-aliased-dep', '100.0.0'),
        },
      },
    },
    specifiers: {
      'pkg-with-1-aliased-dep': '^100.0.0',
    },
  })
})

test('installing the same package via an alias and directly', async () => {
  const project = prepareEmpty()
  const manifest = await addDependenciesToPackage({}, ['negative@npm:is-negative@^1.0.1', 'is-negative@^1.0.1'], await testDefaults({ fastUnpack: false }))

  expect(manifest.dependencies).toStrictEqual({ negative: 'npm:is-negative@^1.0.1', 'is-negative': '^1.0.1' })

  expect(typeof project.requireModule('negative')).toEqual('function')
  expect(typeof project.requireModule('is-negative')).toEqual('function')
})
