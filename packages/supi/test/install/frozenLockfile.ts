import { WANTED_LOCKFILE } from '@pnpm/constants'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import path = require('path')
import sinon = require('sinon')
import {
  install,
  MutatedImporter,
  mutateModules,
} from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import { testDefaults } from '../utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test(`frozen-lockfile: installation fails if specs in package.json don't match the ones in ${WANTED_LOCKFILE}`, async (t) => {
  prepareEmpty(t)

  await install(
    {
      dependencies: {
        'is-positive': '^3.0.0',
      },
    },
    await testDefaults(),
  )

  try {
    await install({
      dependencies: {
        'is-positive': '^3.1.0',
      },
    }, await testDefaults({ frozenLockfile: true }))
    t.fail()
  } catch (err) {
    t.equal(err.message, `Cannot install with "frozen-lockfile" because ${WANTED_LOCKFILE} is not up-to-date with package.json`)
  }
})

test(`frozen-lockfile+shamefully-flatten: installation fails if specs in package.json don't match the ones in ${WANTED_LOCKFILE}`, async (t) => {
  prepareEmpty(t)

  await install({
    dependencies: {
      'is-positive': '^3.0.0',
    },
  }, await testDefaults({ shamefullyFlatten: true }))

  try {
    await install({
      dependencies: {
        'is-positive': '^3.1.0',
      },
    }, await testDefaults({ frozenLockfile: true, shamefullyFlatten: true }))
    t.fail()
  } catch (err) {
    t.equal(err.message, `Cannot install with "frozen-lockfile" because ${WANTED_LOCKFILE} is not up-to-date with package.json`)
  }
})

test(`frozen-lockfile: fail on a shared ${WANTED_LOCKFILE} that does not satisfy one of the package.json files`, async (t) => {
  prepareEmpty(t)

  const importers: MutatedImporter[] = [
    {
      buildIndex: 0,
      mutation: 'install',
      pkg: {
        name: 'p1',

        dependencies: {
          'is-positive': '^3.0.0',
        },
      },
      prefix: path.resolve('p1'),
    },
    {
      buildIndex: 0,
      mutation: 'install',
      pkg: {
        name: 'p2',

        dependencies: {
          'is-negative': '1.0.0',
        },
      },
      prefix: path.resolve('p2'),
    },
  ]
  await mutateModules(importers, await testDefaults())

  importers[0].pkg = {
    dependencies: {
      'is-positive': '^3.1.0',
    },
  }

  try {
    await mutateModules(importers, await testDefaults({ frozenLockfile: true }))
    t.fail()
  } catch (err) {
    t.equal(err.message, `Cannot install with "frozen-lockfile" because ${WANTED_LOCKFILE} is not up-to-date with p1${path.sep}package.json`)
  }
})

test(`frozen-lockfile: should successfully install when ${WANTED_LOCKFILE} is available`, async (t) => {
  const project = prepareEmpty(t)

  const pkg = await install({
    dependencies: {
      'is-positive': '^3.0.0',
    },
  }, await testDefaults({ lockfileOnly: true }))

  await project.hasNot('is-positive')

  await install(pkg, await testDefaults({ frozenLockfile: true }))

  await project.has('is-positive')
})

test(`frozen-lockfile: should fail if no ${WANTED_LOCKFILE} is present`, async (t) => {
  prepareEmpty(t)

  try {
    await install({
      dependencies: {
        'is-positive': '^3.0.0',
      },
    }, await testDefaults({ frozenLockfile: true }))
    t.fail()
  } catch (err) {
    t.equals(err.message, `Headless installation requires a ${WANTED_LOCKFILE} file`)
  }
})

test(`prefer-frozen-lockfile: should prefer headless installation when ${WANTED_LOCKFILE} satisfies package.json`, async (t) => {
  const project = prepareEmpty(t)

  const pkg = await install({
    dependencies: {
      'is-positive': '^3.0.0',
    },
  }, await testDefaults({ lockfileOnly: true }))

  await project.hasNot('is-positive')

  const reporter = sinon.spy()
  await install(pkg, await testDefaults({ reporter, preferFrozenLockfile: true }))

  t.ok(reporter.calledWithMatch({
    level: 'info',
    message: 'Performing headless installation',
    name: 'pnpm',
  }), 'start of headless installation logged')

  await project.has('is-positive')
})

test(`prefer-frozen-lockfile: should not prefer headless installation when ${WANTED_LOCKFILE} does not satisfy package.json`, async (t) => {
  const project = prepareEmpty(t)

  await install({
    dependencies: {
      'is-positive': '^3.0.0',
    },
  }, await testDefaults({ lockfileOnly: true }))

  await project.hasNot('is-positive')

  const reporter = sinon.spy()
  await install({
    dependencies: {
      'is-negative': '1.0.0',
    },
  }, await testDefaults({ reporter, preferFrozenLockfile: true }))

  t.notOk(reporter.calledWithMatch({
    level: 'info',
    message: 'Performing headless installation',
    name: 'pnpm',
  }), 'start of headless installation not logged')

  await project.has('is-negative')
})

test(`prefer-frozen-lockfile: should not fail if no ${WANTED_LOCKFILE} is present and project has no deps`, async (t) => {
  prepareEmpty(t)

  await install({}, await testDefaults({ preferFrozenLockfile: true }))
})

test(`frozen-lockfile: should not fail if no ${WANTED_LOCKFILE} is present and project has no deps`, async (t) => {
  prepareEmpty(t)

  await install({}, await testDefaults({ frozenLockfile: true }))
})

test(`prefer-frozen-lockfile+shamefully-flatten: should prefer headless installation when ${WANTED_LOCKFILE} satisfies package.json`, async (t) => {
  const project = prepareEmpty(t)

  const pkg = await install({
    dependencies: {
      'pkg-with-1-dep': '100.0.0',
    },
  }, await testDefaults({ lockfileOnly: true }))

  await project.hasNot('pkg-with-1-dep')

  const reporter = sinon.spy()
  await install(pkg, await testDefaults({
    preferFrozenLockfile: true,
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

test('prefer-frozen-lockfile: should prefer frozen-lockfile when package has linked dependency', async (t) => {
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

  const importers: MutatedImporter[] = [
    {
      buildIndex: 0,
      mutation: 'install',
      pkg: {
        name: 'p1',

        dependencies: {
          p2: 'link:../p2',
        },
      },
      prefix: path.resolve('p1'),
    },
    {
      buildIndex: 0,
      mutation: 'install',
      pkg: {
        name: 'p2',

        dependencies: {
          'is-negative': '1.0.0',
        },
      },
      prefix: path.resolve('p2'),
    },
  ]
  await mutateModules(importers, await testDefaults())

  const reporter = sinon.spy()
  await mutateModules(importers, await testDefaults({
    preferFrozenLockfile: true,
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
