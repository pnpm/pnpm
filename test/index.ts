import test = require('tape')
import headless from '@pnpm/headless'
import path = require('path')
import testDefaults from './utils/testDefaults'
import isExecutable from './utils/isExecutable'

const fixtures = path.join(__dirname, 'fixtures')

test('installing a simple project', async (t) => {
  const prefix = path.join(fixtures, 'simple')
  await headless(await testDefaults({prefix}))

  t.ok(require(path.join(prefix, 'node_modules', 'is-positive')), 'prod dep installed')
  t.ok(require(path.join(prefix, 'node_modules', 'rimraf')), 'prod dep installed')
  t.ok(require(path.join(prefix, 'node_modules', 'is-negative')), 'dev dep installed')
  t.ok(require(path.join(prefix, 'node_modules', 'colors')), 'optional dep installed')

  await isExecutable(t, path.join(prefix, 'node_modules', '.bin', 'rimraf'))

  t.end()
})
