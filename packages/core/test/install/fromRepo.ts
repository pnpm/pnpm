import path from 'path'
import { RootLog } from '@pnpm/core-loggers'
import { prepareEmpty } from '@pnpm/prepare'
import {
  addDependenciesToPackage,
  install,
} from '@pnpm/core'
import isCI from 'is-ci'
import exists from 'path-exists'
import sinon from 'sinon'
import { testDefaults } from '../utils'

test('from a github repo', async () => {
  const project = prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['kevva/is-negative'], await testDefaults())

  await project.has('is-negative')

  expect(manifest.dependencies).toStrictEqual({
    'is-negative': 'github:kevva/is-negative',
  })
})

test('from a github repo through URL', async () => {
  const project = prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['https://github.com/kevva/is-negative'], await testDefaults())

  await project.has('is-negative')

  expect(manifest.dependencies).toStrictEqual({ 'is-negative': 'github:kevva/is-negative' })
})

test('from a github repo with different name via named installation', async () => {
  const project = prepareEmpty()

  const reporter = sinon.spy()

  const manifest = await addDependenciesToPackage(
    {},
    ['say-hi@github:zkochan/hi#4cdebec76b7b9d1f6e219e06c42d92a6b8ea60cd'],
    await testDefaults({ fastUnpack: false, reporter })
  )

  const m = project.requireModule('say-hi')

  expect(reporter.calledWithMatch({
    added: {
      dependencyType: 'prod',
      name: 'say-hi',
      realName: 'hi',
      version: '1.0.0',
    },
    level: 'debug',
    name: 'pnpm:root',
  } as RootLog)).toBeTruthy()

  expect(m).toEqual('Hi')

  expect(manifest.dependencies).toStrictEqual({ 'say-hi': 'github:zkochan/hi#4cdebec76b7b9d1f6e219e06c42d92a6b8ea60cd' })

  const lockfile = await project.readLockfile()
  expect(lockfile.dependencies).toStrictEqual({
    'say-hi': 'github.com/zkochan/hi/4cdebec76b7b9d1f6e219e06c42d92a6b8ea60cd',
  })

  await project.isExecutable('.bin/hi')
  await project.isExecutable('.bin/szia')
})

// This used to fail. Maybe won't be needed once api/install.ts gets refactored and covered with dedicated unit tests
test('from a github repo with different name', async () => {
  const project = prepareEmpty()

  const reporter = sinon.spy()

  const manifest = await install({
    dependencies: {
      'say-hi': 'github:zkochan/hi#4cdebec76b7b9d1f6e219e06c42d92a6b8ea60cd',
    },
  }, await testDefaults({ fastUnpack: false, reporter }))

  const m = project.requireModule('say-hi')

  expect(reporter.calledWithMatch({
    added: {
      dependencyType: 'prod',
      name: 'say-hi',
      realName: 'hi',
      version: '1.0.0',
    },
    level: 'debug',
    name: 'pnpm:root',
  } as RootLog)).toBeTruthy()

  expect(m).toBe('Hi')

  expect(manifest.dependencies).toStrictEqual({
    'say-hi': 'github:zkochan/hi#4cdebec76b7b9d1f6e219e06c42d92a6b8ea60cd',
  })

  const lockfile = await project.readLockfile()
  expect(lockfile.dependencies).toStrictEqual({
    'say-hi': 'github.com/zkochan/hi/4cdebec76b7b9d1f6e219e06c42d92a6b8ea60cd',
  })

  await project.isExecutable('.bin/hi')
  await project.isExecutable('.bin/szia')
})

test('a subdependency is from a github repo with different name', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['has-aliased-git-dependency'], await testDefaults({ fastUnpack: false }))

  const m = project.requireModule('has-aliased-git-dependency')

  expect(m).toEqual('Hi')

  const lockfile = await project.readLockfile()
  expect(lockfile.packages['/has-aliased-git-dependency/1.0.0'].dependencies).toStrictEqual({
    'has-say-hi-peer': '1.0.0_hi@1.0.0',
    'say-hi': 'github.com/zkochan/hi/4cdebec76b7b9d1f6e219e06c42d92a6b8ea60cd',
  })

  await project.isExecutable('has-aliased-git-dependency/node_modules/.bin/hi')
  await project.isExecutable('has-aliased-git-dependency/node_modules/.bin/szia')

  expect(await exists(path.resolve('node_modules/.pnpm/has-say-hi-peer@1.0.0_hi@1.0.0/node_modules/has-say-hi-peer'))).toBeTruthy()
})

test('from a git repo', async () => {
  if (isCI) {
    console.log('not testing the SSH GIT access via CI')
    return
  }
  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['git+ssh://git@github.com/kevva/is-negative.git'], await testDefaults())

  await project.has('is-negative')
})

// This test is unstable due to dependency on third party registry
// eslint-disable-next-line @typescript-eslint/dot-notation
test.skip('from a non-github git repo', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['git+http://ikt.pm2.io/ikt.git#3325a3e39a502418dc2e2e4bf21529cbbde96228'], await testDefaults())

  const m = project.requireModule('ikt')

  expect(m).toBeTruthy()

  const lockfile = await project.readLockfile()

  const pkgId = 'ikt.pm2.io/ikt/3325a3e39a502418dc2e2e4bf21529cbbde96228'
  expect(lockfile.packages).toHaveProperty([pkgId])
  expect(lockfile.packages[pkgId].resolution).toStrictEqual({
    commit: '3325a3e39a502418dc2e2e4bf21529cbbde96228',
    repo: 'http://ikt.pm2.io/ikt.git',
    type: 'git',
  })
})

test('from a github repo the has no package.json file', async () => {
  const project = prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['pnpm/for-testing.no-package-json'], await testDefaults())

  await project.has('for-testing.no-package-json')

  expect(manifest.dependencies).toStrictEqual({
    'for-testing.no-package-json': 'github:pnpm/for-testing.no-package-json',
  })
})
