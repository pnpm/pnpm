import { prepareEmpty } from '@pnpm/prepare'
import { addDependenciesToPackage } from '@pnpm/core'
import { testDefaults } from '../utils/index.js'

test('blockExoticSubdeps disallows git dependencies in subdependencies', async () => {
  prepareEmpty()

  await expect(addDependenciesToPackage({},
    // @pnpm.e2e/has-aliased-git-dependency has a git-hosted subdependency (say-hi from github:zkochan/hi)
    ['@pnpm.e2e/has-aliased-git-dependency'],
    testDefaults({ blockExoticSubdeps: true, fastUnpack: false })
  )).rejects.toThrow('is not allowed in subdependencies when blockExoticSubdeps is enabled')
})

test('blockExoticSubdeps allows git dependencies in direct dependencies', async () => {
  const project = prepareEmpty()

  // Direct git dependency should be allowed even when blockExoticSubdeps is enabled
  const { updatedManifest: manifest } = await addDependenciesToPackage(
    {},
    ['kevva/is-negative#1.0.0'],
    testDefaults({ blockExoticSubdeps: true })
  )

  project.has('is-negative')

  expect(manifest.dependencies).toStrictEqual({
    'is-negative': 'github:kevva/is-negative#1.0.0',
  })
})

test('blockExoticSubdeps allows registry dependencies in subdependencies', async () => {
  const project = prepareEmpty()

  // A package with only registry subdependencies should work fine
  await addDependenciesToPackage(
    {},
    ['is-positive@1.0.0'],
    testDefaults({ blockExoticSubdeps: true })
  )

  project.has('is-positive')
})

test('blockExoticSubdeps: false (default) allows git dependencies in subdependencies', async () => {
  const project = prepareEmpty()

  // Without blockExoticSubdeps (or with it set to false), git subdeps should be allowed
  await addDependenciesToPackage(
    {},
    ['@pnpm.e2e/has-aliased-git-dependency'],
    testDefaults({ blockExoticSubdeps: false, fastUnpack: false })
  )

  const m = project.requireModule('@pnpm.e2e/has-aliased-git-dependency')
  expect(m).toBe('Hi')
})
