import assertProject from '@pnpm/assert-project'
import test = require('tape')
import headless from '@pnpm/headless'
import path = require('path')
import exists = require('path-exists')
import rimraf = require('rimraf-then')
import testDefaults from './utils/testDefaults'

const fixtures = path.join(__dirname, 'fixtures')

test('installing a simple project', async (t) => {
  const prefix = path.join(fixtures, 'simple')
  await headless(await testDefaults({prefix}))

  const project = assertProject(t, prefix)
  t.ok(project.requireModule('is-positive'), 'prod dep installed')
  t.ok(project.requireModule('rimraf'), 'prod dep installed')
  t.ok(project.requireModule('is-negative'), 'dev dep installed')
  t.ok(project.requireModule('colors'), 'optional dep installed')

  await project.isExecutable('.bin/rimraf')

  t.end()
})

test('run pre/postinstall scripts', async (t) => {
  const prefix = path.join(fixtures, 'deps-have-lifecycle-scripts')
  const outputJsonPath = path.join(prefix, 'output.json')
  await rimraf(outputJsonPath)

  await headless(await testDefaults({prefix}))

  const project = assertProject(t, prefix)
  const generatedByPreinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-preinstall')
  t.ok(typeof generatedByPreinstall === 'function', 'generatedByPreinstall() is available')

  const generatedByPostinstall = project.requireModule('pre-and-postinstall-scripts-example/generated-by-postinstall')
  t.ok(typeof generatedByPostinstall === 'function', 'generatedByPostinstall() is available')

  t.deepEqual(require(outputJsonPath), ['install', 'postinstall'])

  await rimraf(outputJsonPath)
  await rimraf(path.join(prefix, 'node_modules'))

  await headless(await testDefaults({prefix, ignoreScripts: true}))

  t.notOk(await exists(outputJsonPath))

  t.end()
})
