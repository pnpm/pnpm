import { prepareEmpty } from '@pnpm/prepare'
import { addDependenciesToPackage } from '@pnpm/core'
import { testDefaults } from '../utils/index.js'

test('registrySubdepsOnly disallows git dependencies in subdependencies', async () => {
  prepareEmpty()

  await expect(addDependenciesToPackage({},
    // @pnpm.e2e/has-aliased-git-dependency has a git-hosted subdependency (say-hi from github:zkochan/hi)
    ['@pnpm.e2e/has-aliased-git-dependency'],
    testDefaults({ registrySubdepsOnly: true, fastUnpack: false })
  )).rejects.toThrow('is not allowed in subdependencies when registrySubdepsOnly is enabled')
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
