import { DeprecationLog } from '@pnpm/core-loggers'
import prepare from '@pnpm/prepare'
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
  const project = prepare(t)

  {
    const reporter = sinon.spy()

    await addDependenciesToPackage(['express@0.14.1'], await testDefaults({ reporter }))

    t.ok(reporter.calledWithMatch({
      deprecated: 'express 0.x series is deprecated',
      level: 'debug',
      name: 'pnpm:deprecation',
      pkgId: 'localhost+4873/express/0.14.1',
    } as DeprecationLog), 'deprecation warning reported')
  }

  const shr = await project.loadShrinkwrap()
  t.equal(
    shr.packages['/express/0.14.1'].deprecated,
    'express 0.x series is deprecated',
    'deprecated field added to shrinkwrap.yaml',
  )

  {
    const reporter = sinon.spy()

    await addDependenciesToPackage(['express@4.16.3'], await testDefaults({ reporter }))

    t.notOk(reporter.calledWithMatch({
      level: 'debug',
      name: 'pnpm:deprecation',
      pkgId: 'localhost+4873/express/4.16.3',
    } as DeprecationLog), 'deprecation warning reported')
  }
})
