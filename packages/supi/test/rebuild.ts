import prepare, { preparePackages } from '@pnpm/prepare'
import ncpCB = require('ncp')
import path = require('path')
import exists = require('path-exists')
import {
  addDependenciesToPackage,
  mutateModules,
  rebuild,
  rebuildPkgs,
} from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import { promisify } from 'util'
import {
  pathToLocalPkg,
  testDefaults,
} from './utils'

const ncp = promisify(ncpCB.ncp)
const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test('rebuilds dependencies', async (t: tape.Test) => {
  const project = prepare(t)

  const pkgs = ['pre-and-postinstall-scripts-example', 'zkochan/install-scripts-example#prepare']
  await addDependenciesToPackage(pkgs, await testDefaults({ targetDependenciesField: 'devDependencies', ignoreScripts: true }))

  let modules = await project.loadModules()
  t.deepEqual(modules!.pendingBuilds, [
    '/pre-and-postinstall-scripts-example/1.0.0',
    'github.com/zkochan/install-scripts-example/2de638b8b572cd1e87b74f4540754145fb2c0ebb',
  ])

  await rebuild([{ buildIndex: 0, prefix: process.cwd() }], await testDefaults())

  modules = await project.loadModules()
  t.ok(modules)
  t.equal(modules!.pendingBuilds.length, 0)

  {
    t.notOk(await exists('node_modules/pre-and-postinstall-scripts-example/generated-by-prepare.js'))
    t.ok(await exists('node_modules/pre-and-postinstall-scripts-example/generated-by-preinstall.js'))

    const generatedByPreinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-preinstall')
    t.ok(typeof generatedByPreinstall === 'function', 'generatedByPreinstall() is available')

    const generatedByPostinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-postinstall')
    t.ok(typeof generatedByPostinstall === 'function', 'generatedByPostinstall() is available')
  }

  {
    const scripts = project.requireModule('install-scripts-example-for-pnpm/output.json')
    t.equal(scripts[0], 'preinstall')
    t.equal(scripts[1], 'install')
    t.equal(scripts[2], 'postinstall')
    t.equal(scripts[3], 'prepare')
  }
})

test('rebuild does not fail when a linked package is present', async (t: tape.Test) => {
  const project = prepare(t)
  await ncp(pathToLocalPkg('local-pkg'), path.resolve('..', 'local-pkg'))

  await addDependenciesToPackage(['link:../local-pkg', 'is-positive'], await testDefaults())

  await rebuild([{ buildIndex: 0, prefix: process.cwd() }], await testDefaults())

  // see related issue https://github.com/pnpm/pnpm/issues/1155
  t.pass('rebuild did not fail')
})

test('rebuilds specific dependencies', async (t: tape.Test) => {
  const project = prepare(t)
  await addDependenciesToPackage([
    'pre-and-postinstall-scripts-example',
    'zkochan/install-scripts-example'
  ], await testDefaults({ targetDependenciesField: 'devDependencies', ignoreScripts: true }))

  await rebuildPkgs([{ prefix: process.cwd() }], ['install-scripts-example-for-pnpm'], await testDefaults())

  await project.hasNot('pre-and-postinstall-scripts-example/generated-by-preinstall')
  await project.hasNot('pre-and-postinstall-scripts-example/generated-by-postinstall')

  const generatedByPreinstall = project.requireModule('install-scripts-example-for-pnpm/generated-by-preinstall')
  t.ok(typeof generatedByPreinstall === 'function', 'generatedByPreinstall() is available')

  const generatedByPostinstall = project.requireModule('install-scripts-example-for-pnpm/generated-by-postinstall')
  t.ok(typeof generatedByPostinstall === 'function', 'generatedByPostinstall() is available')
})

test('rebuild with pending option', async (t: tape.Test) => {
  const project = prepare(t)
  await addDependenciesToPackage(['pre-and-postinstall-scripts-example'], await testDefaults({ ignoreScripts: true }))
  await addDependenciesToPackage(['zkochan/install-scripts-example'], await testDefaults({ ignoreScripts: true }))

  let modules = await project.loadModules()
  t.deepEqual(modules!.pendingBuilds, [
    '/pre-and-postinstall-scripts-example/1.0.0',
    'github.com/zkochan/install-scripts-example/6d879afcee10ece4d3f0e8c09de2993232f3430a',
  ])

  await project.hasNot('pre-and-postinstall-scripts-example/generated-by-preinstall')
  await project.hasNot('pre-and-postinstall-scripts-example/generated-by-postinstall')

  await project.hasNot('install-scripts-example-for-pnpm/generated-by-preinstall')
  await project.hasNot('install-scripts-example-for-pnpm/generated-by-postinstall')

  await rebuild([{ buildIndex: 0, prefix: process.cwd() }], await testDefaults({ rawNpmConfig: { pending: true } }))

  modules = await project.loadModules()
  t.ok(modules)
  t.equal(modules!.pendingBuilds.length, 0)

  {
    const generatedByPreinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-preinstall')
    t.ok(typeof generatedByPreinstall === 'function', 'generatedByPreinstall() is available')

    const generatedByPostinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-postinstall')
    t.ok(typeof generatedByPostinstall === 'function', 'generatedByPostinstall() is available')
  }

  {
    const generatedByPreinstall = project.requireModule('install-scripts-example-for-pnpm/generated-by-preinstall')
    t.ok(typeof generatedByPreinstall === 'function', 'generatedByPreinstall() is available')

    const generatedByPostinstall = project.requireModule('install-scripts-example-for-pnpm/generated-by-postinstall')
    t.ok(typeof generatedByPostinstall === 'function', 'generatedByPostinstall() is available')
  }
})

test('rebuild dependencies in correct order', async (t: tape.Test) => {
  const project = prepare(t)

  await addDependenciesToPackage(['with-postinstall-a'], await testDefaults({ ignoreScripts: true }))

  let modules = await project.loadModules()
  t.ok(modules)
  t.doesNotEqual(modules!.pendingBuilds.length, 0)

  await project.hasNot('.localhost+4873/with-postinstall-b/1.0.0/node_modules/with-postinstall-b/output.json')
  await project.hasNot('with-postinstall-a/output.json')

  await rebuild([{ buildIndex: 0, prefix: process.cwd() }], await testDefaults({ rawNpmConfig: { pending: true } }))

  modules = await project.loadModules()
  t.ok(modules)
  t.equal(modules!.pendingBuilds.length, 0)

  t.ok(+project.requireModule('.localhost+4873/with-postinstall-b/1.0.0/node_modules/with-postinstall-b/output.json')[0] < +project.requireModule('with-postinstall-a/output.json')[0])
})

test('rebuild dependencies in correct order when node_modules uses independent-leaves', async (t: tape.Test) => {
  const project = prepare(t)

  await addDependenciesToPackage(['with-postinstall-a'], await testDefaults({ ignoreScripts: true, independentLeaves: true }))

  let modules = await project.loadModules()
  t.ok(modules)
  t.doesNotEqual(modules!.pendingBuilds.length, 0)

  await project.hasNot('.localhost+4873/with-postinstall-b/1.0.0/node_modules/with-postinstall-b/output.json')
  await project.hasNot('with-postinstall-a/output.json')

  await rebuild([{ buildIndex: 0, prefix: process.cwd() }], await testDefaults({ rawNpmConfig: { pending: true }, independentLeaves: true }))

  modules = await project.loadModules()
  t.ok(modules)
  t.equal(modules!.pendingBuilds.length, 0)

  t.ok(+project.requireModule('.localhost+4873/with-postinstall-b/1.0.0/node_modules/with-postinstall-b/output.json')[0] < +project.requireModule('with-postinstall-a/output.json')[0])
})

test('rebuild multiple packages in correct order', async (t: tape.Test) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
      },
      scripts: {
        postinstall: `node -e "process.stdout.write('project-1')" | json-append ../output1.json && node -e "process.stdout.write('project-1')" | json-append ../output2.json`,
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
        'project-1': '1'
      },
      scripts: {
        postinstall: `node -e "process.stdout.write('project-2')" | json-append ../output1.json`,
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        'json-append': '1',
        'project-1': '1'
      },
      scripts: {
        postinstall: `node -e "process.stdout.write('project-3')" | json-append ../output2.json`,
      },
    },
    {
      name: 'project-0',
      version: '1.0.0',

      dependencies: {},
    },
  ])

  const importers = [
    {
      buildIndex: 1,
      prefix: path.resolve('project-3'),
    },
    {
      buildIndex: 1,
      prefix: path.resolve('project-2'),
    },
    {
      buildIndex: 0,
      prefix: path.resolve('project-1'),
    },
    {
      buildIndex: 0,
      prefix: path.resolve('project-0'),
    },
  ]
  await mutateModules(
    importers.map((importer) => ({ ...importer, mutation: 'install' as 'install' })),
    await testDefaults({ ignoreScripts: true }),
  )

  await rebuild(importers, await testDefaults())

  const outputs1 = await import(path.resolve('output1.json')) as string[]
  const outputs2 = await import(path.resolve('output2.json')) as string[]

  t.deepEqual(outputs1, ['project-1', 'project-2'])
  t.deepEqual(outputs2, ['project-1', 'project-3'])
})

test('rebuild links bins', async (t: tape.Test) => {
  const project = prepare(t)

  await addDependenciesToPackage(['has-generated-bins-as-dep', 'generated-bins'], await testDefaults({ ignoreScripts: true }))

  t.notOk(await exists(path.resolve('node_modules/.bin/cmd1')))
  t.notOk(await exists(path.resolve('node_modules/.bin/cmd2')))

  t.ok(await exists(path.resolve('node_modules/has-generated-bins-as-dep/package.json')))
  t.notOk(await exists(path.resolve('node_modules/has-generated-bins-as-dep/node_modules/.bin/cmd1')))
  t.notOk(await exists(path.resolve('node_modules/has-generated-bins-as-dep/node_modules/.bin/cmd2')))

  await rebuild([{ buildIndex: 0, prefix: process.cwd() }], await testDefaults({ rawNpmConfig: { pending: true } }))

  await project.isExecutable('.bin/cmd1')
  await project.isExecutable('.bin/cmd2')
  await project.isExecutable('has-generated-bins-as-dep/node_modules/.bin/cmd1')
  await project.isExecutable('has-generated-bins-as-dep/node_modules/.bin/cmd2')
})
