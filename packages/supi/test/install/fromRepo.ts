import { RootLog } from '@pnpm/core-loggers'
import prepare from '@pnpm/prepare'
import isCI = require('is-ci')
import path = require('path')
import exists = require('path-exists')
import readPkg = require('read-pkg')
import sinon = require('sinon')
import {
  addDependenciesToPackage,
  install,
} from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import { testDefaults } from '../utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test('from a github repo', async (t: tape.Test) => {
  const project = prepare(t)
  await addDependenciesToPackage(['kevva/is-negative'], await testDefaults())

  const m = project.requireModule('is-negative')

  t.ok(m, 'isNegative() is available')

  const pkgJson = await readPkg()
  t.deepEqual(pkgJson.dependencies, { 'is-negative': 'github:kevva/is-negative' }, 'has been added to dependencies in package.json')
})

test('from a github repo with different name via named installation', async (t: tape.Test) => {
  const project = prepare(t)

  const reporter = sinon.spy()

  await addDependenciesToPackage(['say-hi@github:zkochan/hi#4cdebec76b7b9d1f6e219e06c42d92a6b8ea60cd'], await testDefaults({ reporter }))

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

  const pkgJson = await readPkg()
  t.deepEqual(pkgJson.dependencies, { 'say-hi': 'github:zkochan/hi#4cdebec76b7b9d1f6e219e06c42d92a6b8ea60cd' }, 'has been added to dependencies in package.json')

  const shr = await project.loadShrinkwrap()
  t.deepEqual(shr.dependencies, {
    'say-hi': 'github.com/zkochan/hi/4cdebec76b7b9d1f6e219e06c42d92a6b8ea60cd',
  }, 'the aliased name added to shrinkwrap.yaml')

  await project.isExecutable('.bin/hi')
  await project.isExecutable('.bin/szia')
})

// This used to fail. Maybe won't be needed once api/install.ts gets refactored and covered with dedicated unit tests
test('from a github repo with different name', async (t: tape.Test) => {
  const project = prepare(t, {
    dependencies: {
      'say-hi': 'github:zkochan/hi#4cdebec76b7b9d1f6e219e06c42d92a6b8ea60cd',
    },
  })

  const reporter = sinon.spy()

  await install(await testDefaults({ reporter }))

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

  const pkgJson = await readPkg()
  t.deepEqual(pkgJson.dependencies, { 'say-hi': 'github:zkochan/hi#4cdebec76b7b9d1f6e219e06c42d92a6b8ea60cd' }, 'has been added to dependencies in package.json')

  const shr = await project.loadShrinkwrap()
  t.deepEqual(shr.dependencies, {
    'say-hi': 'github.com/zkochan/hi/4cdebec76b7b9d1f6e219e06c42d92a6b8ea60cd',
  }, 'the aliased name added to shrinkwrap.yaml')

  await project.isExecutable('.bin/hi')
  await project.isExecutable('.bin/szia')
})

test('a subdependency is from a github repo with different name', async (t: tape.Test) => {
  const project = prepare(t)

  await addDependenciesToPackage(['has-aliased-git-dependency'], await testDefaults())

  const m = project.requireModule('has-aliased-git-dependency')

  t.equal(m, 'Hi', 'subdep is accessible')

  const shr = await project.loadShrinkwrap()
  t.deepEqual(shr.packages['/has-aliased-git-dependency/1.0.0'].dependencies, {
    'has-say-hi-peer': '/has-say-hi-peer/1.0.0/say-hi@1.0.0',
    'say-hi': 'github.com/zkochan/hi/4cdebec76b7b9d1f6e219e06c42d92a6b8ea60cd',
  }, 'the aliased name added to shrinkwrap.yaml')

  await project.isExecutable('has-aliased-git-dependency/node_modules/.bin/hi')
  await project.isExecutable('has-aliased-git-dependency/node_modules/.bin/szia')

  t.ok(await exists(path.join('node_modules', '.localhost+4873', 'has-say-hi-peer', '1.0.0', 'say-hi@1.0.0', 'node_modules', 'has-say-hi-peer')),
    'aliased name used to resolve a peer dependency')
})

test('from a git repo', async (t: tape.Test) => {
  if (isCI) {
    t.skip('not testing the SSH GIT access via CI')
    return t.end()
  }
  const project = prepare(t)
  await addDependenciesToPackage(['git+ssh://git@github.com/kevva/is-negative.git'], await testDefaults())

  const m = project.requireModule('is-negative')

  t.ok(m, 'isNegative() is available')
})

// This test started failing on Travis for some reason
// so skipping it on Travis
const isTravis = process.env.TRAVIS === 'true'
if (!isTravis) {
  test('from a non-github git repo', async (t: tape.Test) => {
    const project = prepare(t)

    await addDependenciesToPackage(['git+http://ikt.pm2.io/ikt.git#3325a3e39a502418dc2e2e4bf21529cbbde96228'], await testDefaults())

    const m = project.requireModule('ikt')

    t.ok(m, 'ikt is available')

    const shr = await project.loadShrinkwrap()

    const pkgId = 'ikt.pm2.io/ikt/3325a3e39a502418dc2e2e4bf21529cbbde96228'
    t.ok(shr.packages[pkgId])
    t.deepEqual(shr.packages[pkgId].resolution, {
      commit: '3325a3e39a502418dc2e2e4bf21529cbbde96228',
      repo: 'http://ikt.pm2.io/ikt.git',
      type: 'git',
    })
  })
}
