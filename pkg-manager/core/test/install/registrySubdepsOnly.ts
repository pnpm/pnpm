import { type PnpmError } from '@pnpm/error'
import { prepareEmpty } from '@pnpm/prepare'
import {
  addDependenciesToPackage,
  install,
} from '@pnpm/core'
import { testDefaults } from '../utils/index.js'

test('registrySubdepsOnly disallows git dependencies in subdependencies', async () => {
  prepareEmpty()

  let err!: PnpmError
  try {
    // @pnpm.e2e/has-aliased-git-dependency has a git-hosted subdependency (say-hi from github:zkochan/hi)
    await addDependenciesToPackage(
      {},
      ['@pnpm.e2e/has-aliased-git-dependency'],
      testDefaults({ registrySubdepsOnly: true, fastUnpack: false })
    )
    throw new Error('installation should have failed')
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }

  expect(err.code).toBe('ERR_PNPM_NON_REGISTRY_SUBDEPENDENCY')
  expect(err.message).toContain('is not allowed in subdependencies when registrySubdepsOnly is enabled')
})

test('registrySubdepsOnly allows git dependencies in direct dependencies', async () => {
  const project = prepareEmpty()

  // Direct git dependency should be allowed even when registrySubdepsOnly is enabled
  const { updatedManifest: manifest } = await addDependenciesToPackage(
    {},
    ['kevva/is-negative#1.0.0'],
    testDefaults({ registrySubdepsOnly: true })
  )

  project.has('is-negative')

  expect(manifest.dependencies).toStrictEqual({
    'is-negative': 'github:kevva/is-negative#1.0.0',
  })
})

test('registrySubdepsOnly allows registry dependencies in subdependencies', async () => {
  const project = prepareEmpty()

  // A package with only registry subdependencies should work fine
  await addDependenciesToPackage(
    {},
    ['is-positive@1.0.0'],
    testDefaults({ registrySubdepsOnly: true })
  )

  project.has('is-positive')
})

test('registrySubdepsOnly: false (default) allows git dependencies in subdependencies', async () => {
  const project = prepareEmpty()

  // Without registrySubdepsOnly (or with it set to false), git subdeps should be allowed
  await addDependenciesToPackage(
    {},
    ['@pnpm.e2e/has-aliased-git-dependency'],
    testDefaults({ registrySubdepsOnly: false, fastUnpack: false })
  )

  const m = project.requireModule('@pnpm.e2e/has-aliased-git-dependency')
  expect(m).toBe('Hi')
})

test('registrySubdepsOnly set via pnpm settings in manifest', async () => {
  prepareEmpty()

  let err!: PnpmError
  try {
    await install(
      {
        dependencies: {
          '@pnpm.e2e/has-aliased-git-dependency': '1.0.0',
        },
        pnpm: {
          registrySubdepsOnly: true,
        },
      },
      testDefaults({ registrySubdepsOnly: true, fastUnpack: false })
    )
    throw new Error('installation should have failed')
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }

  expect(err.code).toBe('ERR_PNPM_NON_REGISTRY_SUBDEPENDENCY')
})
