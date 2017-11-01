import tape = require('tape')
import promisifyTape from 'tape-promise'
import {prepare, testDefaults} from '../utils'
import {installPkgs} from 'supi'
import sinon = require('sinon')
import {DeprecationLog} from 'pnpm-logger'
const test = promisifyTape(tape)

// TODO: use a smaller package for testing deprecation
test('reports warning when installing deprecated packages', async (t: tape.Test) => {
  const project = prepare(t)

  const reporter = sinon.spy()

  await installPkgs(['jade@1.11.0'], testDefaults({reporter}))

  t.ok(reporter.calledWithMatch(<DeprecationLog>{
    name: 'pnpm:deprecation',
    level: 'warn',
    pkgId: 'localhost+4873/jade/1.11.0',
    deprecated: 'Jade has been renamed to pug, please install the latest version of pug instead of jade',
  }), 'deprecation warning reported')

  const shr = await project.loadShrinkwrap()
  t.equal(
    shr.packages['/jade/1.11.0'].deprecated,
    'Jade has been renamed to pug, please install the latest version of pug instead of jade',
    'deprecated field added to shrinkwrap.yaml'
  )
})
