import { WANTED_LOCKFILE } from '@pnpm/constants'
import { RootLog } from '@pnpm/core-loggers'
import { prepareEmpty } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import isCI = require('is-ci')
import path = require('path')
import exists = require('path-exists')
import sinon = require('sinon')
import {
  addDependenciesToPackage,
  install,
} from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import { testDefaults } from '../utils'

const test = promisifyTape(tape)

test('from a github repo', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  const manifest = await addDependenciesToPackage({}, ['kevva/is-negative'], await testDefaults())

  await project.has('is-negative')

  t.deepEqual(manifest.dependencies, { 'is-negative': 'github:kevva/is-negative' }, 'has been added to dependencies in package.json')
})

test('from a github repo with different name via named installation', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  const reporter = sinon.spy()

  const manifest = await addDependenciesToPackage(
    {},
    ['say-hi@github:zkochan/hi#4cdebec76b7b9d1f6e219e06c42d92a6b8ea60cd'],
    await testDefaults({ fastUnpack: false, reporter }),
  )

  const m = project.requireModule('say-hi')

  t.ok(reporter.calledWithMatch({
    added: {
      dependencyType: 'prod',
      name: 'say-hi',
      realName: 'hi',
      version: '1.0.0',
    },
    level: 'debug',
    name: 'pnpm:root',
  } as RootLog), 'adding to root logged with real name and alias name')

  t.equal(m, 'Hi', 'dep is available')

  t.deepEqual(manifest.dependencies, { 'say-hi': 'github:zkochan/hi#4cdebec76b7b9d1f6e219e06c42d92a6b8ea60cd' }, 'has been added to dependencies in package.json')

  const lockfile = await project.readLockfile()
  t.deepEqual(lockfile.dependencies, {
    'say-hi': 'github.com/zkochan/hi/4cdebec76b7b9d1f6e219e06c42d92a6b8ea60cd',
  }, `the aliased name added to ${WANTED_LOCKFILE}`)

  await project.isExecutable('.bin/hi')
  await project.isExecutable('.bin/szia')
})

// This used to fail. Maybe won't be needed once api/install.ts gets refactored and covered with dedicated unit tests
test('from a github repo with different name', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  const reporter = sinon.spy()

  const manifest = await install({
    dependencies: {
      'say-hi': 'github:zkochan/hi#4cdebec76b7b9d1f6e219e06c42d92a6b8ea60cd',
    },
  }, await testDefaults({ fastUnpack: false, reporter }))

  const m = project.requireModule('say-hi')

  t.ok(reporter.calledWithMatch({
    added: {
      dependencyType: 'prod',
      name: 'say-hi',
      realName: 'hi',
      version: '1.0.0',
    },
    level: 'debug',
    name: 'pnpm:root',
  } as RootLog), 'adding to root logged with real name and alias name')

  t.equal(m, 'Hi', 'dep is available')

  t.deepEqual(manifest.dependencies, { 'say-hi': 'github:zkochan/hi#4cdebec76b7b9d1f6e219e06c42d92a6b8ea60cd' }, 'has been added to dependencies in package.json')

  const lockfile = await project.readLockfile()
  t.deepEqual(lockfile.dependencies, {
    'say-hi': 'github.com/zkochan/hi/4cdebec76b7b9d1f6e219e06c42d92a6b8ea60cd',
  }, `the aliased name added to ${WANTED_LOCKFILE}`)

  await project.isExecutable('.bin/hi')
  await project.isExecutable('.bin/szia')
})

test('a subdependency is from a github repo with different name', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await addDependenciesToPackage({}, ['has-aliased-git-dependency'], await testDefaults({ fastUnpack: false }))

  const m = project.requireModule('has-aliased-git-dependency')

  t.equal(m, 'Hi', 'subdep is accessible')

  const lockfile = await project.readLockfile()
  t.deepEqual(lockfile.packages['/has-aliased-git-dependency/1.0.0'].dependencies, {
    'has-say-hi-peer': '1.0.0_say-hi@1.0.0',
    'say-hi': 'github.com/zkochan/hi/4cdebec76b7b9d1f6e219e06c42d92a6b8ea60cd',
  }, `the aliased name added to ${WANTED_LOCKFILE}`)

  await project.isExecutable('has-aliased-git-dependency/node_modules/.bin/hi')
  await project.isExecutable('has-aliased-git-dependency/node_modules/.bin/szia')

  t.ok(await exists(path.resolve(`node_modules/.pnpm/localhost+${REGISTRY_MOCK_PORT}/has-say-hi-peer@1.0.0_say-hi@1.0.0/node_modules/has-say-hi-peer`)),
    'aliased name used to resolve a peer dependency')
})

test('from a git repo', async (t: tape.Test) => {
  if (isCI) {
    t.skip('not testing the SSH GIT access via CI')
    return t.end()
  }
  const project = prepareEmpty(t)
  await addDependenciesToPackage({}, ['git+ssh://git@github.com/kevva/is-negative.git'], await testDefaults())

  await project.has('is-negative')
})

// This test is unstable due to dependency on third party registry
// tslint:disable-next-line:no-string-literal
test.skip('from a non-github git repo', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await addDependenciesToPackage({}, ['git+http://ikt.pm2.io/ikt.git#3325a3e39a502418dc2e2e4bf21529cbbde96228'], await testDefaults())

  const m = project.requireModule('ikt')

  t.ok(m, 'ikt is available')

  const lockfile = await project.readLockfile()

  const pkgId = 'ikt.pm2.io/ikt/3325a3e39a502418dc2e2e4bf21529cbbde96228'
  t.ok(lockfile.packages[pkgId])
  t.deepEqual(lockfile.packages[pkgId].resolution, {
    commit: '3325a3e39a502418dc2e2e4bf21529cbbde96228',
    repo: 'http://ikt.pm2.io/ikt.git',
    type: 'git',
  })
})
