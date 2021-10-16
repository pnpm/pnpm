import path from 'path'
import { RootLog } from '@pnpm/core-loggers'
import { prepareEmpty } from '@pnpm/prepare'
import { pathToLocalPkg } from '@pnpm/test-fixtures'
import {
  addDependenciesToPackage,
  install,
  link,
  mutateModules,
} from '@pnpm/core'
import sinon from 'sinon'
import { testDefaults } from './utils'

test('prune removes extraneous packages', async () => {
  const project = prepareEmpty()

  const opts = await testDefaults()
  let manifest = await addDependenciesToPackage({}, ['is-negative@2.1.0'], { ...opts, targetDependenciesField: 'dependencies' })
  manifest = await addDependenciesToPackage(manifest, ['applyq@0.2.1'], { ...opts, targetDependenciesField: 'devDependencies' })
  manifest = await addDependenciesToPackage(manifest, ['fnumber@0.1.0'], { ...opts, targetDependenciesField: 'optionalDependencies' })
  manifest = await addDependenciesToPackage(manifest, ['is-positive@2.0.0', '@zkochan/logger@0.1.0'], opts)
  manifest = await link([pathToLocalPkg('hello-world-js-bin')], path.resolve(process.cwd(), 'node_modules'), { ...opts, manifest, dir: process.cwd() })

  await project.has('hello-world-js-bin') // external link added

  delete manifest.dependencies!['is-positive']
  delete manifest.dependencies!['@zkochan/logger']

  const reporter = sinon.spy()

  await mutateModules(
    [
      {
        buildIndex: 0,
        manifest,
        mutation: 'install',
        pruneDirectDependencies: true,
        rootDir: process.cwd(),
      },
    ],
    {
      ...opts,
      pruneStore: true,
      reporter,
    }
  )

  expect(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:root',
    removed: {
      dependencyType: undefined,
      name: 'hello-world-js-bin',
      version: '1.0.0',
    },
  } as RootLog)).toBeTruthy()

  await project.hasNot('hello-world-js-bin') // external link pruned

  await project.storeHasNot('is-positive', '2.0.0')
  await project.hasNot('is-positive')

  await project.storeHasNot('@zkochan/logger', '0.1.0')
  await project.hasNot('@zkochan/logger')

  await project.storeHas('is-negative', '2.1.0')
  await project.has('is-negative')

  await project.storeHas('applyq', '0.2.1')
  await project.has('applyq')

  await project.storeHas('fnumber', '0.1.0')
  await project.has('fnumber')
})

test('prune removes dev dependencies in production', async () => {
  const project = prepareEmpty()

  let manifest = await addDependenciesToPackage({}, ['is-positive@2.0.0'], await testDefaults({ targetDependenciesField: 'devDependencies' }))
  manifest = await addDependenciesToPackage(manifest, ['is-negative@2.1.0'], await testDefaults({ targetDependenciesField: 'dependencies' }))
  manifest = await addDependenciesToPackage(manifest, ['fnumber@0.1.0'], await testDefaults({ targetDependenciesField: 'optionalDependencies' }))
  await install(manifest, await testDefaults({
    include: {
      dependencies: true,
      devDependencies: false,
      optionalDependencies: true,
    },
    pruneStore: true,
  }))

  await project.storeHasNot('is-positive', '2.0.0')
  await project.hasNot('is-positive')

  await project.storeHas('is-negative', '2.1.0')
  await project.has('is-negative')

  await project.storeHas('fnumber', '0.1.0')
  await project.has('fnumber')
})
