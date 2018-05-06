import deepRequireCwd = require('deep-require-cwd')
import loadYamlFile = require('load-yaml-file')
import path = require('path')
import exists = require('path-exists')
import sinon = require('sinon')
import {install, installPkgs} from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import {
  addDistTag,
  local,
  pathToLocalPkg,
  prepare,
  testDefaults,
} from '../utils'

const test = promisifyTape(tape)

test('successfully install optional dependency with subdependencies', async (t) => {
  const project = prepare(t)

  await installPkgs(['fsevents@1.0.14'], await testDefaults({saveOptional: true}))
})

test('skip failing optional dependencies', async (t: tape.Test) => {
  const project = prepare(t)
  await installPkgs(['pkg-with-failing-optional-dependency@1.0.1'], await testDefaults())

  const m = project.requireModule('pkg-with-failing-optional-dependency')
  t.ok(m(-1), 'package with failed optional dependency has the dependencies installed correctly')
})

test('skip non-existing optional dependency', async (t: tape.Test) => {
  const project = prepare(t, {
    dependencies: {
      'is-positive': '*',
    },
    optionalDependencies: {
      'i-do-not-exist': '1000',
    },
  })

  const reporter = sinon.spy()
  await install(await testDefaults({reporter}))

  t.ok(reporter.calledWithMatch({
    package: {
      name: 'i-do-not-exist',
      version: '1000',
    },
    parents: [],
    reason: 'resolution_failure',
  }), 'warning reported')

  const m = project.requireModule('is-positive')
  t.ok(m, 'installation succeded')
})

test('skip optional dependency that does not support the current OS', async (t: tape.Test) => {
  const project = prepare(t, {
    optionalDependencies: {
      'not-compatible-with-any-os': '*',
    },
  })
  const reporter = sinon.spy()

  await install(await testDefaults({reporter}))

  await project.hasNot('not-compatible-with-any-os')
  await project.storeHas('not-compatible-with-any-os', '1.0.0')
  t.notOk(await exists(path.resolve('node_modules', '.localhost+4873', 'dep-of-optional-pkg', '1.0.0')), "isn't linked into node_modules")

  const shr = await project.loadShrinkwrap()
  t.ok(shr.packages['/not-compatible-with-any-os/1.0.0'], 'shrinkwrap contains optional dependency')
  t.ok(shr.packages['/dep-of-optional-pkg/1.0.0'], 'shrinkwrap contains dependency of optional dependency')

  const modulesInfo = await loadYamlFile<{skipped: string[]}>(path.join('node_modules', '.modules.yaml'))
  t.deepEquals(modulesInfo.skipped, [
    'localhost+4873/dep-of-optional-pkg/1.0.0',
    'localhost+4873/not-compatible-with-any-os/1.0.0',
  ])

  const logMatcher = sinon.match({
    package: {
      id: 'localhost+4873/not-compatible-with-any-os/1.0.0',
      name: 'not-compatible-with-any-os',
      version: '1.0.0',
    },
    parents: [],
    reason: 'incompatible_engine',
  })
  const reportedTimes = reporter.withArgs(logMatcher).callCount
  t.equal(reportedTimes, 1, 'skipping optional dependency is logged')
})

test('skip optional dependency that does not support the current Node version', async (t: tape.Test) => {
  const project = prepare(t, {
    optionalDependencies: {
      'for-legacy-node': '*',
    },
  })
  const reporter = sinon.spy()

  await install(await testDefaults({reporter}))

  await project.hasNot('for-legacy-node')
  await project.storeHas('for-legacy-node', '1.0.0')

  const logMatcher = sinon.match({
    package: {
      id: 'localhost+4873/for-legacy-node/1.0.0',
      name: 'for-legacy-node',
      version: '1.0.0',
    },
    parents: [],
    reason: 'incompatible_engine',
  })
  const reportedTimes = reporter.withArgs(logMatcher).callCount
  t.equal(reportedTimes, 1, 'skipping optional dependency is logged')
})

test('skip optional dependency that does not support the current pnpm version', async (t: tape.Test) => {
  const project = prepare(t, {
    optionalDependencies: {
      'for-legacy-pnpm': '*',
    },
  })
  const reporter = sinon.spy()

  await install(await testDefaults({reporter}))

  await project.hasNot('for-legacy-pnpm')
  await project.storeHas('for-legacy-pnpm', '1.0.0')

  const logMatcher = sinon.match({
    package: {
      id: 'localhost+4873/for-legacy-pnpm/1.0.0',
      name: 'for-legacy-pnpm',
      version: '1.0.0',
    },
    parents: [],
    reason: 'incompatible_engine',
  })
  const reportedTimes = reporter.withArgs(logMatcher).callCount
  t.equal(reportedTimes, 1, 'skipping optional dependency is logged')
})

test('don\'t skip optional dependency that does not support the current OS when forcing', async (t) => {
  const project = prepare(t, {
    optionalDependencies: {
      'not-compatible-with-any-os': '*',
    },
  })

  await install(await testDefaults({
    force: true,
  }))

  await project.has('not-compatible-with-any-os')
  await project.storeHas('not-compatible-with-any-os', '1.0.0')
})

test('optional subdependency is skipped', async (t: tape.Test) => {
  const project = prepare(t)
  const reporter = sinon.spy()

  await installPkgs(['pkg-with-optional', 'dep-of-optional-pkg'], await testDefaults({reporter}))

  const modulesInfo = await loadYamlFile<{skipped: string[]}>(path.join('node_modules', '.modules.yaml'))

  t.deepEqual(modulesInfo.skipped, ['localhost+4873/not-compatible-with-any-os/1.0.0'], 'optional subdep skipped')

  const logMatcher = sinon.match({
    package: {
      id: 'localhost+4873/not-compatible-with-any-os/1.0.0',
      name: 'not-compatible-with-any-os',
      version: '1.0.0',
    },
    parents: [
      {
        id: 'localhost+4873/pkg-with-optional/1.0.0',
        name: 'pkg-with-optional',
        version: '1.0.0',
      },
    ],
    reason: 'incompatible_engine',
  })
  const reportedTimes = reporter.withArgs(logMatcher).callCount
  t.equal(reportedTimes, 1, 'skipping optional dependency is logged')
})

test('not installing optional dependencies when optional is false', async (t: tape.Test) => {
  const project = prepare(t, {
    dependencies: {
      'pkg-with-good-optional': '*',
    },
    optionalDependencies: {
      'is-positive': '1.0.0',
    },
  })

  await install(await testDefaults({optional: false}))

  await project.hasNot('is-positive')
  await project.has('pkg-with-good-optional')

  t.ok(deepRequireCwd(['pkg-with-good-optional', 'dep-of-pkg-with-1-dep', './package.json']))
  t.notOk(deepRequireCwd.silent(['pkg-with-good-optional', 'is-positive', './package.json']), 'optional subdep not installed')
})

test('optional dependency has bigger priority than regular dependency', async (t: tape.Test) => {
  const project = prepare(t, {
    dependencies: {
      'is-positive': '1.0.0',
    },
    optionalDependencies: {
      'is-positive': '3.1.0',
    },
  })

  await install(await testDefaults())

  t.ok(deepRequireCwd(['is-positive', './package.json']).version, '3.1.0')
})
