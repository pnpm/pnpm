import path from 'path'
import { readModulesManifest, writeModulesManifest } from '@pnpm/modules-yaml'
import { prepareEmpty } from '@pnpm/prepare'
import {
  addDependenciesToPackage,
  install,
  mutateModulesInSingleProject,
} from '@pnpm/core'
import { type ProjectRootDir } from '@pnpm/types'
import { testDefaults } from '../utils'

test('the modules cache is pruned when it expires', async () => {
  const project = prepareEmpty()

  let manifest = await install({
    dependencies: {
      'is-positive': '1.0.0',
      'is-negative': '1.0.0',
    },
  }, testDefaults())

  const modulesDir = path.resolve('node_modules')
  const modulesFile = await readModulesManifest(modulesDir)!

  expect(modulesFile?.prunedAt).toBeTruthy()

  manifest = (await mutateModulesInSingleProject({
    dependencyNames: ['is-negative'],
    manifest,
    mutation: 'uninstallSome',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({}))).manifest

  project.has('.pnpm/is-negative@1.0.0/node_modules/is-negative')

  const prunedAt = new Date()
  prunedAt.setMinutes(prunedAt.getMinutes() - 3)
  modulesFile!.prunedAt = prunedAt.toString()
  await writeModulesManifest(modulesDir, modulesFile as any) // eslint-disable-line

  await addDependenciesToPackage(manifest,
    ['is-negative@2.0.0'],
    testDefaults({ modulesCacheMaxAge: 2 })
  )

  project.hasNot('.pnpm/is-negative@1.0.0/node_modules/is-negative')
})

test('the modules cache is pruned when it expires and headless install is used', async () => {
  const project = prepareEmpty()

  let manifest = await install({
    dependencies: {
      'is-positive': '1.0.0',
      'is-negative': '1.0.0',
    },
  }, testDefaults())

  const modulesDir = path.resolve('node_modules')
  const modulesFile = await readModulesManifest(modulesDir)

  expect(modulesFile?.prunedAt).toBeTruthy()

  manifest = (await mutateModulesInSingleProject({
    dependencyNames: ['is-negative'],
    manifest,
    mutation: 'uninstallSome',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({ lockfileOnly: true }))).manifest

  manifest = await install(manifest, testDefaults({ frozenLockfile: true }))

  project.has('.pnpm/is-negative@1.0.0/node_modules/is-negative')

  const prunedAt = new Date()
  prunedAt.setMinutes(prunedAt.getMinutes() - 3)
  modulesFile!.prunedAt = prunedAt.toString()
  await writeModulesManifest(modulesDir, modulesFile as any) // eslint-disable-line

  await install(manifest, testDefaults({ frozenLockfile: true, modulesCacheMaxAge: 2 }))

  project.hasNot('.pnpm/is-negative@1.0.0/node_modules/is-negative')
})
