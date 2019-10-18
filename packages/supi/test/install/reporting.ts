import { WANTED_LOCKFILE } from '@pnpm/constants'
import { DeprecationLog } from '@pnpm/core-loggers'
import { prepareEmpty } from '@pnpm/prepare'
import sinon = require('sinon')
import {
  addDependenciesToPackage,
} from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import { testDefaults } from '../utils'

const test = promisifyTape(tape)

// TODO: use a smaller package for testing deprecation
test('reports warning when installing deprecated packages', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  let manifest
  {
    const reporter = sinon.spy()

    manifest = await addDependenciesToPackage({}, ['express@0.14.1'], await testDefaults({ fastUnpack: false, reporter }))

    t.ok(reporter.calledWithMatch({
      deprecated: 'express 0.x series is deprecated',
      level: 'debug',
      name: 'pnpm:deprecation',
      pkgId: 'localhost+4873/express/0.14.1',
    } as DeprecationLog), 'deprecation warning reported')
  }

  const lockfile = await project.readLockfile()
  t.equal(
    lockfile.packages['/express/0.14.1'].deprecated,
    'express 0.x series is deprecated',
    `deprecated field added to ${WANTED_LOCKFILE}`,
  )

  {
    const reporter = sinon.spy()

    await addDependenciesToPackage(manifest, ['express@4.16.3'], await testDefaults({ fastUnpack: false, reporter }))

    t.notOk(reporter.calledWithMatch({
      level: 'debug',
      name: 'pnpm:deprecation',
      pkgId: 'localhost+4873/express/4.16.3',
    } as DeprecationLog), 'deprecation warning reported')
  }
})
