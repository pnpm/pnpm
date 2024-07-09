import path from 'path'
import { type RootLog } from '@pnpm/core-loggers'
import { prepareEmpty } from '@pnpm/prepare'
import { fixtures } from '@pnpm/test-fixtures'
import {
  addDependenciesToPackage,
  install,
  link,
  mutateModulesInSingleProject,
} from '@pnpm/core'
import { type ProjectRootDir } from '@pnpm/types'
import sinon from 'sinon'
import { testDefaults } from './utils'

const f = fixtures(__dirname)

test('prune removes extraneous packages', async () => {
  const linkedPkg = f.prepare('hello-world-js-bin')
  const project = prepareEmpty()

  const opts = testDefaults()
  let manifest = await addDependenciesToPackage({}, ['is-negative@2.1.0'], { ...opts, targetDependenciesField: 'dependencies' })
  manifest = await addDependenciesToPackage(manifest, ['applyq@0.2.1'], { ...opts, targetDependenciesField: 'devDependencies' })
  manifest = await addDependenciesToPackage(manifest, ['fnumber@0.1.0'], { ...opts, targetDependenciesField: 'optionalDependencies' })
  manifest = await addDependenciesToPackage(manifest, ['is-positive@2.0.0', '@zkochan/logger@0.1.0'], opts)
  manifest = await link([linkedPkg], path.resolve('node_modules'), { ...opts, manifest, dir: process.cwd() })

  project.has('@pnpm.e2e/hello-world-js-bin') // external link added

  delete manifest.dependencies!['is-positive']
  delete manifest.dependencies!['@zkochan/logger']

  const reporter = sinon.spy()

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    pruneDirectDependencies: true,
    rootDir: process.cwd() as ProjectRootDir,
  }, {
    ...opts,
    pruneStore: true,
    reporter,
  })

  expect(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:root',
    removed: {
      dependencyType: undefined,
      name: '@pnpm.e2e/hello-world-js-bin',
      version: '1.0.0',
    },
  } as RootLog)).toBeTruthy()

  project.hasNot('@pnpm.e2e/hello-world-js-bin') // external link pruned

  project.storeHasNot('is-positive', '2.0.0')
  project.hasNot('is-positive')

  project.storeHasNot('@zkochan/logger', '0.1.0')
  project.hasNot('@zkochan/logger')

  project.storeHas('is-negative', '2.1.0')
  project.has('is-negative')

  project.storeHas('applyq', '0.2.1')
  project.has('applyq')

  project.storeHas('fnumber', '0.1.0')
  project.has('fnumber')
})

test('prune removes dev dependencies in production', async () => {
  const project = prepareEmpty()

  let manifest = await addDependenciesToPackage({}, ['is-positive@2.0.0'], testDefaults({ targetDependenciesField: 'devDependencies' }))
  manifest = await addDependenciesToPackage(manifest, ['is-negative@2.1.0'], testDefaults({ targetDependenciesField: 'dependencies' }))
  manifest = await addDependenciesToPackage(manifest, ['fnumber@0.1.0'], testDefaults({ targetDependenciesField: 'optionalDependencies' }))
  await install(manifest, testDefaults({
    include: {
      dependencies: true,
      devDependencies: false,
      optionalDependencies: true,
    },
    pruneStore: true,
  }))

  project.storeHasNot('is-positive', '2.0.0')
  project.hasNot('is-positive')

  project.storeHas('is-negative', '2.1.0')
  project.has('is-negative')

  project.storeHas('fnumber', '0.1.0')
  project.has('fnumber')
})
