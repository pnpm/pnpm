import prepare, { preparePackages } from '@pnpm/prepare'
import path = require('path')
import sinon = require('sinon')
import { install } from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import writeJsonFile from 'write-json-file'
import { testDefaults } from '../utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test("frozen-shrinkwrap: installation fails if specs in package.json don't match the ones in shrinkwrap.yaml", async (t) => {
  const project = prepare(t, {
    dependencies: {
      'is-positive': '^3.0.0',
    },
  })

  await install(await testDefaults())

  await writeJsonFile('package.json', {
    dependencies: {
      'is-positive': '^3.1.0',
    },
  })

  try {
    await install(await testDefaults({ frozenShrinkwrap: true }))
    t.fail()
  } catch (err) {
    t.equal(err.message, 'Cannot install with "frozen-shrinkwrap" because shrinkwrap.yaml is not up-to-date with package.json')
  }
})

test("frozen-shrinkwrap+shamefully-flatten: installation fails if specs in package.json don't match the ones in shrinkwrap.yaml", async (t) => {
  const project = prepare(t, {
    dependencies: {
      'is-positive': '^3.0.0',
    },
  })

  await install(await testDefaults({ shamefullyFlatten: true }))

  await writeJsonFile('package.json', {
    dependencies: {
      'is-positive': '^3.1.0',
    },
  })

  try {
    await install(await testDefaults({ frozenShrinkwrap: true, shamefullyFlatten: true }))
    t.fail()
  } catch (err) {
    t.equal(err.message, 'Cannot install with "frozen-shrinkwrap" because shrinkwrap.yaml is not up-to-date with package.json')
  }
})

test('frozen-shrinkwrap: fail on a shared shrinkwrap.yaml that does not satisfy one of the package.json files', async (t) => {
  const project = preparePackages(t, [
    {
      name: 'p1',
      dependencies: {
        'is-positive': '^3.0.0',
      },
    },
    {
      name: 'p2',
      dependencies: {
        'is-negative': '1.0.0',
      },
    },
  ])

  const importers = [
    {
      prefix: path.resolve('p1'),
    },
    {
      prefix: path.resolve('p2'),
    },
  ]
  await install(await testDefaults({ importers }))

  await writeJsonFile('p1/package.json', {
    dependencies: {
      'is-positive': '^3.1.0',
    },
  })

  try {
    await install(await testDefaults({ frozenShrinkwrap: true, importers }))
    t.fail()
  } catch (err) {
    t.equal(err.message, `Cannot install with "frozen-shrinkwrap" because shrinkwrap.yaml is not up-to-date with p1${path.sep}package.json`)
  }
})

test('frozen-shrinkwrap: should successfully install when shrinkwrap.yaml is available', async (t) => {
  const project = prepare(t, {
    dependencies: {
      'is-positive': '^3.0.0',
    },
  })

  await install(await testDefaults({ shrinkwrapOnly: true }))

  await project.hasNot('is-positive')

  await install(await testDefaults({ frozenShrinkwrap: true }))

  await project.has('is-positive')
})

test('frozen-shrinkwrap: should fail if no shrinkwrap.yaml is present', async (t) => {
  prepare(t, {
    dependencies: {
      'is-positive': '^3.0.0',
    },
  })

  try {
    await install(await testDefaults({ frozenShrinkwrap: true }))
    t.fail()
  } catch (err) {
    t.equals(err.message, 'Headless installation requires a shrinkwrap.yaml file')
  }
})

test('prefer-frozen-shrinkwrap: should prefer headless installation when shrinkwrap.yaml satisfies package.json', async (t) => {
  const project = prepare(t, {
    dependencies: {
      'is-positive': '^3.0.0',
    },
  })

  await install(await testDefaults({ shrinkwrapOnly: true }))

  await project.hasNot('is-positive')

  const reporter = sinon.spy()
  await install(await testDefaults({ reporter, preferFrozenShrinkwrap: true }))

  t.ok(reporter.calledWithMatch({
    level: 'info',
    message: 'Performing headless installation',
    name: 'pnpm',
  }), 'start of headless installation logged')

  await project.has('is-positive')
})

test('prefer-frozen-shrinkwrap: should not prefer headless installation when shrinkwrap.yaml does not satisfy package.json', async (t) => {
  const project = prepare(t, {
    dependencies: {
      'is-positive': '^3.0.0',
    },
  })

  await install(await testDefaults({ shrinkwrapOnly: true }))

  await project.writePackageJson({
    dependencies: {
      'is-negative': '1.0.0',
    },
  })

  await project.hasNot('is-positive')

  const reporter = sinon.spy()
  await install(await testDefaults({ reporter, preferFrozenShrinkwrap: true }))

  t.notOk(reporter.calledWithMatch({
    level: 'info',
    message: 'Performing headless installation',
    name: 'pnpm',
  }), 'start of headless installation not logged')

  await project.has('is-negative')
})

test('prefer-frozen-shrinkwrap: should not fail if no shrinkwrap.yaml is present and project has no deps', async (t) => {
  const project = prepare(t)

  await install(await testDefaults({ preferFrozenShrinkwrap: true }))
})

test('frozen-shrinkwrap: should not fail if no shrinkwrap.yaml is present and project has no deps', async (t) => {
  const project = prepare(t)

  await install(await testDefaults({ frozenShrinkwrap: true }))
})

test('prefer-frozen-shrinkwrap+shamefully-flatten: should prefer headless installation when shrinkwrap.yaml satisfies package.json', async (t) => {
  const project = prepare(t, {
    dependencies: {
      'pkg-with-1-dep': '100.0.0',
    },
  })

  await install(await testDefaults({ shrinkwrapOnly: true }))

  await project.hasNot('pkg-with-1-dep')

  const reporter = sinon.spy()
  await install(await testDefaults({
    preferFrozenShrinkwrap: true,
    reporter,
    shamefullyFlatten: true,
  }))

  t.ok(reporter.calledWithMatch({
    level: 'info',
    message: 'Performing headless installation',
    name: 'pnpm',
  }), 'start of headless installation logged')

  await project.has('pkg-with-1-dep')
  await project.has('dep-of-pkg-with-1-dep')
})

test('prefer-frozen-shrinkwrap: should prefer frozen-shrinkwrap when package has linked dependency', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'p1',
      dependencies: {
        p2: 'link:../p2',
      },
    },
    {
      name: 'p2',
      dependencies: {
        'is-negative': '1.0.0',
      },
    },
  ])

  const importers = [
    {
      prefix: path.resolve('p1'),
    },
    {
      prefix: path.resolve('p2'),
    },
  ]
  await install(await testDefaults({ importers }))

  const reporter = sinon.spy()
  await install(await testDefaults({
    importers,
    preferFrozenShrinkwrap: true,
    reporter,
  }))

  t.ok(reporter.calledWithMatch({
    level: 'info',
    message: 'Performing headless installation',
    name: 'pnpm',
  }), 'start of headless installation logged')

  await projects['p1'].has('p2')
  await projects['p2'].has('is-negative')
})
