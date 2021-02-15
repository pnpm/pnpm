import path from 'path'
import { write as writeModulesYaml } from '@pnpm/modules-yaml'
import { prepareEmpty } from '@pnpm/prepare'
import {
  addDependenciesToPackage,
  install,
  mutateModules,
} from 'supi'
import { testDefaults } from '../utils'

test('the modules cache is pruned when it expires', async () => {
  const project = prepareEmpty()

  let manifest = await install({
    dependencies: {
      'is-positive': '1.0.0',
      'is-negative': '1.0.0',
    },
  }, await testDefaults())

  const modulesFile = await project.readModulesManifest()

  expect(modulesFile?.prunedAt).toBeTruthy()

  manifest = (await mutateModules([
    {
      dependencyNames: ['is-negative'],
      manifest,
      mutation: 'uninstallSome',
      rootDir: process.cwd(),
    },
  ], await testDefaults({})))[0].manifest

  await project.has('.pnpm/is-negative@1.0.0/node_modules/is-negative')

  const prunedAt = new Date()
  prunedAt.setMinutes(prunedAt.getMinutes() - 3)
  modulesFile!.prunedAt = prunedAt.toString()
  await writeModulesYaml(path.resolve('node_modules'), modulesFile as any) // eslint-disable-line

  await addDependenciesToPackage(manifest,
    ['is-negative@2.0.0'],
    await testDefaults({ modulesCacheMaxAge: 2 })
  )

  await project.hasNot('.pnpm/is-negative@1.0.0/node_modules/is-negative')
})

test('the modules cache is pruned when it expires and headless install is used', async () => {
  const project = prepareEmpty()

  let manifest = await install({
    dependencies: {
      'is-positive': '1.0.0',
      'is-negative': '1.0.0',
    },
  }, await testDefaults())

  const modulesFile = await project.readModulesManifest()

  expect(modulesFile?.prunedAt).toBeTruthy()

  manifest = (await mutateModules([
    {
      dependencyNames: ['is-negative'],
      manifest,
      mutation: 'uninstallSome',
      rootDir: process.cwd(),
    },
  ], await testDefaults({ lockfileOnly: true })))[0].manifest

  manifest = await install(manifest, await testDefaults({ frozenLockfile: true }))

  await project.has('.pnpm/is-negative@1.0.0/node_modules/is-negative')

  const prunedAt = new Date()
  prunedAt.setMinutes(prunedAt.getMinutes() - 3)
  modulesFile!.prunedAt = prunedAt.toString()
  await writeModulesYaml(path.resolve('node_modules'), modulesFile as any) // eslint-disable-line

  await install(manifest, await testDefaults({ frozenLockfile: true, modulesCacheMaxAge: 2 }))

  await project.hasNot('.pnpm/is-negative@1.0.0/node_modules/is-negative')
})
