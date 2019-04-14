import { RootLog } from '@pnpm/core-loggers'
import { prepareEmpty } from '@pnpm/prepare'
import path = require('path')
import sinon = require('sinon')
import {
  addDependenciesToPackage,
  install,
  link,
  mutateModules,
} from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import { pathToLocalPkg, testDefaults } from './utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test('prune removes extraneous packages', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  const opts = await testDefaults()
  let pkg = await addDependenciesToPackage({}, ['is-negative@2.1.0'], { ...opts, targetDependenciesField: 'dependencies' })
  pkg = await addDependenciesToPackage(pkg, ['applyq@0.2.1'], { ...opts, targetDependenciesField: 'devDependencies' })
  pkg = await addDependenciesToPackage(pkg, ['fnumber@0.1.0'], { ...opts, targetDependenciesField: 'optionalDependencies' })
  pkg = await addDependenciesToPackage(pkg, ['is-positive@2.0.0', '@zkochan/logger@0.1.0'], opts)
  pkg = await link([pathToLocalPkg('hello-world-js-bin')], path.resolve(process.cwd(), 'node_modules'), { ...opts, pkg, prefix: process.cwd() })

  await project.has('hello-world-js-bin') // external link added

  delete pkg.dependencies!['is-positive']
  delete pkg.dependencies!['@zkochan/logger']

  const reporter = sinon.spy()

  await mutateModules(
    [
      {
        buildIndex: 0,
        mutation: 'install',
        pkg,
        prefix: process.cwd(),
        pruneDirectDependencies: true,
      },
    ],
    {
      ...opts,
      pruneStore: true,
      reporter,
    },
  )

  t.ok(reporter.calledWithMatch({
    level: 'debug',
    name: 'pnpm:root',
    removed: {
      dependencyType: undefined,
      name: 'hello-world-js-bin',
      version: '1.0.0',
    },
  } as RootLog), 'removing link to external package')

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

test('prune removes dev dependencies in production', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  let pkg = await addDependenciesToPackage({}, ['is-positive@2.0.0'], await testDefaults({ targetDependenciesField: 'devDependencies' }))
  pkg = await addDependenciesToPackage(pkg, ['is-negative@2.1.0'], await testDefaults({ targetDependenciesField: 'dependencies' }))
  pkg = await addDependenciesToPackage(pkg, ['fnumber@0.1.0'], await testDefaults({ targetDependenciesField: 'optionalDependencies' }))
  await install(pkg, await testDefaults({
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
