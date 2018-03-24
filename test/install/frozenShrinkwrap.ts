import sinon = require('sinon')
import {install, installPkgs, uninstall} from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import writeJsonFile = require('write-json-file')
import {
  prepare,
  testDefaults,
} from '../utils'

const test = promisifyTape(tape)

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
    await install(await testDefaults({frozenShrinkwrap: true}))
    t.fail()
  } catch (err) {
    t.equal(err.message, 'Cannot run headless installation because shrinkwrap.yaml is not up-to-date with package.json')
  }
})

test('frozen-shrinkwrap: should successfully install when shrinkwrap.yaml is available', async (t) => {
  const project = prepare(t, {
    dependencies: {
      'is-positive': '^3.0.0',
    },
  })

  await install(await testDefaults({shrinkwrapOnly: true}))

  project.hasNot('is-positive')

  await install(await testDefaults({frozenShrinkwrap: true}))

  project.has('is-positive')
})

test('frozen-shrinkwrap: should fail if no shrinkwrap.yaml is present', async (t) => {
  prepare(t, {
    dependencies: {
      'is-positive': '^3.0.0',
    },
  })

  try {
    await install(await testDefaults({frozenShrinkwrap: true}))
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

  await install(await testDefaults({shrinkwrapOnly: true}))

  project.hasNot('is-positive')

  const reporter = sinon.spy()
  await install(await testDefaults({reporter, preferFrozenShrinkwrap: true}))

  t.ok(reporter.calledWithMatch({
    level: 'info',
    message: 'Performing headless installation',
    name: 'pnpm',
  }), 'start of headless installation logged')

  project.has('is-positive')
})

test('prefer-frozen-shrinkwrap: should not prefer headless installation when shrinkwrap.yaml does not satisfy package.json', async (t) => {
  const project = prepare(t, {
    dependencies: {
      'is-positive': '^3.0.0',
    },
  })

  await install(await testDefaults({shrinkwrapOnly: true}))

  await project.writePackageJson({
    dependencies: {
      'is-negative': '1.0.0',
    },
  })

  project.hasNot('is-positive')

  const reporter = sinon.spy()
  await install(await testDefaults({reporter, preferFrozenShrinkwrap: true}))

  t.notOk(reporter.calledWithMatch({
    level: 'info',
    message: 'Performing headless installation',
    name: 'pnpm',
  }), 'start of headless installation not logged')

  project.has('is-negative')
})
