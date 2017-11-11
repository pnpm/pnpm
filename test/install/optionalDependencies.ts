import path = require('path')
import tape = require('tape')
import promisifyTape from 'tape-promise'
import loadYamlFile = require('load-yaml-file')
import exists = require('path-exists')
import deepRequireCwd = require('deep-require-cwd')
import sinon = require('sinon')
import {install, installPkgs} from 'supi'
import {
  prepare,
  addDistTag,
  testDefaults,
  pathToLocalPkg,
  local,
} from '../utils'

const test = promisifyTape(tape)

test('successfully install optional dependency with subdependencies', async function (t) {
  const project = prepare(t)

  await installPkgs(['fsevents@1.0.14'], testDefaults({saveOptional: true}))
})

test('skip failing optional dependencies', async (t: tape.Test) => {
  const project = prepare(t)
  await installPkgs(['pkg-with-failing-optional-dependency@1.0.1'], testDefaults())

  const m = project.requireModule('pkg-with-failing-optional-dependency')
  t.ok(m(-1), 'package with failed optional dependency has the dependencies installed correctly')
})

test('skip optional dependency that does not support the current OS', async (t: tape.Test) => {
  const project = prepare(t, {
    optionalDependencies: {
      'not-compatible-with-any-os': '*'
    }
  })
  const reporter = sinon.spy()

  await install(testDefaults({reporter}))

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
    level: 'warn',
    message: 'Skipping failed optional dependency not-compatible-with-any-os@1.0.0',
  })
  const reportedTimes = reporter.withArgs(logMatcher).callCount
  t.equal(reportedTimes, 1, 'skipping optional dependency is logged')
})

test('skip optional dependency that does not support the current Node version', async (t: tape.Test) => {
  const project = prepare(t, {
    optionalDependencies: {
      'for-legacy-node': '*'
    }
  })
  const reporter = sinon.spy()

  await install(testDefaults({reporter}))

  await project.hasNot('for-legacy-node')
  await project.storeHas('for-legacy-node', '1.0.0')

  const logMatcher = sinon.match({
    level: 'warn',
    message: 'Skipping failed optional dependency for-legacy-node@1.0.0',
  })
  const reportedTimes = reporter.withArgs(logMatcher).callCount
  t.equal(reportedTimes, 1, 'skipping optional dependency is logged')
})

test('skip optional dependency that does not support the current pnpm version', async (t: tape.Test) => {
  const project = prepare(t, {
    optionalDependencies: {
      'for-legacy-pnpm': '*'
    }
  })
  const reporter = sinon.spy()

  await install(testDefaults({reporter}))

  await project.hasNot('for-legacy-pnpm')
  await project.storeHas('for-legacy-pnpm', '1.0.0')

  const logMatcher = sinon.match({
    level: 'warn',
    message: 'Skipping failed optional dependency for-legacy-pnpm@1.0.0',
  })
  const reportedTimes = reporter.withArgs(logMatcher).callCount
  t.equal(reportedTimes, 1, 'skipping optional dependency is logged')
})

test('don\'t skip optional dependency that does not support the current OS when forcing', async function (t) {
  const project = prepare(t, {
    optionalDependencies: {
      'not-compatible-with-any-os': '*'
    }
  })

  await install(testDefaults({
    force: true
  }))

  await project.has('not-compatible-with-any-os')
  await project.storeHas('not-compatible-with-any-os', '1.0.0')
})

test('optional subdependency is skipped', async (t: tape.Test) => {
  const project = prepare(t)
  const reporter = sinon.spy()

  await installPkgs(['pkg-with-optional', 'dep-of-optional-pkg'], testDefaults({reporter}))

  const modulesInfo = await loadYamlFile<{skipped: string[]}>(path.join('node_modules', '.modules.yaml'))

  t.deepEqual(modulesInfo.skipped, ['localhost+4873/not-compatible-with-any-os/1.0.0'], 'optional subdep skipped')

  const logMatcher = sinon.match({
    level: 'warn',
    message: 'pkg-with-optional: Skipping failed optional dependency not-compatible-with-any-os@1.0.0',
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

  await install(testDefaults({optional: false}))

  project.hasNot('is-positive')
  project.has('pkg-with-good-optional')

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

  await install(testDefaults())

  t.ok(deepRequireCwd(['is-positive', './package.json']).version, '3.1.0')
})
