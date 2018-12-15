import { RootLog } from '@pnpm/core-loggers'
import prepare from '@pnpm/prepare'
import path = require('path')
import readPkg = require('read-pkg')
import sinon = require('sinon')
import {
  addDependenciesToPackage,
  install,
  link,
  mutateModules,
} from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import writePkg = require('write-pkg')
import { pathToLocalPkg, testDefaults } from './utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test('prune removes extraneous packages', async (t: tape.Test) => {
  const project = prepare(t)

  const opts = await testDefaults()
  await addDependenciesToPackage(['is-negative@2.1.0'], { ...opts, targetDependenciesField: 'dependencies' })
  await addDependenciesToPackage(['applyq@0.2.1'], { ...opts, targetDependenciesField: 'devDependencies' })
  await addDependenciesToPackage(['fnumber@0.1.0'], { ...opts, targetDependenciesField: 'optionalDependencies' })
  await addDependenciesToPackage(['is-positive@2.0.0', '@zkochan/logger@0.1.0'], opts)
  await link([pathToLocalPkg('hello-world-js-bin')], path.resolve(process.cwd(), 'node_modules'), opts)

  await project.has('hello-world-js-bin') // external link added

  const pkg = await readPkg()

  delete pkg.dependencies['is-positive']
  delete pkg.dependencies['@zkochan/logger']

  await writePkg(pkg)

  const reporter = sinon.spy()

  await mutateModules(
    [
      {
        mutation: 'install',
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
  const project = prepare(t)

  await addDependenciesToPackage(['is-positive@2.0.0'], await testDefaults({ targetDependenciesField: 'devDependencies' }))
  await addDependenciesToPackage(['is-negative@2.1.0'], await testDefaults({ targetDependenciesField: 'dependencies' }))
  await addDependenciesToPackage(['fnumber@0.1.0'], await testDefaults({ targetDependenciesField: 'optionalDependencies' }))
  await install(await testDefaults({
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
