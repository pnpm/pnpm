import { WANTED_LOCKFILE } from '@pnpm/constants'
import prepare from '@pnpm/prepare'
import fs = require('mz/fs')
import path = require('path')
import { install } from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import { testDefaults } from '../utils'

const test = promisifyTape(tape)

test('production install (with --production flag)', async (t: tape.Test) => {
  const project = prepare(t, {
    dependencies: {
      rimraf: '2.6.2',
    },
    devDependencies: {
      '@rstacruz/tap-spec': '4.1.1',
      'once': '^1.4.0', // once is also a transitive dependency of rimraf
    },
  })

  await install(await testDefaults({
    include: {
      dependencies: true,
      devDependencies: false,
      optionalDependencies: true,
    },
  }))

  const rimraf = project.requireModule('rimraf')

  let tapStatErrCode: number = 0
  try {
    fs.statSync(path.resolve('node_modules', '@rstacruz'))
  } catch (err) {
    tapStatErrCode = err.code
  }

  t.ok(rimraf, 'rimraf exists')
  t.is(tapStatErrCode, 'ENOENT', 'tap-spec does not exist')
})

test('install dev dependencies only', async (t: tape.Test) => {
  const project = prepare(t, {
    dependencies: {
      'is-positive': '1.0.0',
      'once': '^1.4.0',
    },
    devDependencies: {
      inflight: '1.0.6',
    },
  })

  await install(await testDefaults({
    include: {
      dependencies: false,
      devDependencies: true,
      optionalDependencies: false,
    },
  }))

  const inflight = project.requireModule('inflight')
  t.equal(typeof inflight, 'function', 'dev dependency is available')

  await project.hasNot('once')

  {
    const shr = await project.loadShrinkwrap()
    t.ok(shr.packages['/is-positive/1.0.0'].dev === false)
  }

  {
    const currentShrinkwrap = await project.loadCurrentShrinkwrap()
    t.notOk(currentShrinkwrap.packages['/is-positive/1.0.0'], `prod dep only not added to current ${WANTED_LOCKFILE}`)
  }

  // Repeat normal installation adds missing deps to node_modules
  await install(await testDefaults())

  await project.has('once')

  {
    const currentShrinkwrap = await project.loadCurrentShrinkwrap()
    t.ok(currentShrinkwrap.packages['/is-positive/1.0.0'], `prod dep added to current ${WANTED_LOCKFILE}`)
  }
})

test('fail if installing different types of dependencies in a project that uses an external shrinkwrap', async (t: tape.Test) => {
  const project = prepare(t, {
    dependencies: {
      'is-positive': '1.0.0',
      'once': '^1.4.0',
    },
    devDependencies: {
      inflight: '1.0.6',
    },
  })

  const lockfileDirectory = path.resolve('..')

  await install(await testDefaults({
    include: {
      dependencies: false,
      devDependencies: true,
      optionalDependencies: false,
    },
    lockfileDirectory,
  }))

  const inflight = project.requireModule('inflight')
  t.equal(typeof inflight, 'function', 'dev dependency is available')

  await project.hasNot('once')

  let err!: Error & { code: string }

  try {
    await install(await testDefaults({
      include: {
        dependencies: true,
        devDependencies: true,
        optionalDependencies: true,
      },
      lockfileDirectory,
    }))
  } catch (_) {
    err = _
  }

  t.ok(err, 'installation failed')
  t.equal(err.code, 'ERR_PNPM_INCLUDED_DEPS_CONFLICT', 'error has correct error code')
  t.ok(err.message.indexOf('was installed with devDependencies. Current install wants optionalDependencies, dependencies, devDependencies.') !== -1, 'correct error message')
})
