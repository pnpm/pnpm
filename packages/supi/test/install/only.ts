import { WANTED_LOCKFILE } from '@pnpm/constants'
import { prepareEmpty } from '@pnpm/prepare'
import fs = require('mz/fs')
import path = require('path')
import { install } from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import { testDefaults } from '../utils'

const test = promisifyTape(tape)

test('production install (with --production flag)', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await install({
    dependencies: {
      'pkg-with-1-dep': '100.0.0',
    },
    devDependencies: {
      '@zkochan/foo': '1.0.0',
      'once': '^1.4.0', // once is also a transitive dependency of rimraf
    },
  }, await testDefaults({
    include: {
      dependencies: true,
      devDependencies: false,
      optionalDependencies: true,
    },
  }))

  await project.has('pkg-with-1-dep')
  await project.hasNot('@zkochan/foo')
})

test('install dev dependencies only', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  const manifest = await install({
    dependencies: {
      'is-positive': '1.0.0',
      'once': '^1.4.0',
    },
    devDependencies: {
      inflight: '1.0.6',
    },
  }, await testDefaults({
    include: {
      dependencies: false,
      devDependencies: true,
      optionalDependencies: false,
    },
  }))

  await project.has('inflight')
  await project.hasNot('once')

  {
    const lockfile = await project.readLockfile()
    t.ok(lockfile.packages['/is-positive/1.0.0'].dev === false)
  }

  {
    const currentLockfile = await project.readCurrentLockfile()
    t.notOk(currentLockfile.packages['/is-positive/1.0.0'], `prod dep only not added to current ${WANTED_LOCKFILE}`)
  }

  // Repeat normal installation adds missing deps to node_modules
  await install(manifest, await testDefaults())

  await project.has('once')

  {
    const currentLockfile = await project.readCurrentLockfile()
    t.ok(currentLockfile.packages['/is-positive/1.0.0'], `prod dep added to current ${WANTED_LOCKFILE}`)
  }
})

test('fail if installing different types of dependencies in a project that uses an external lockfile', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  const lockfileDirectory = path.resolve('..')

  const manifest = await install({
    dependencies: {
      'is-positive': '1.0.0',
      'once': '^1.4.0',
    },
    devDependencies: {
      inflight: '1.0.6',
    },
  }, await testDefaults({
    include: {
      dependencies: false,
      devDependencies: true,
      optionalDependencies: false,
    },
    lockfileDirectory,
  }))

  await project.has('inflight')
  await project.hasNot('once')

  let err!: Error & { code: string }

  try {
    await install(manifest, await testDefaults({
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
  t.ok(err.message.includes('was installed with devDependencies. Current install wants optionalDependencies, dependencies, devDependencies.'), 'correct error message')
})
