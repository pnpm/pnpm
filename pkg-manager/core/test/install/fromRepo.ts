import path from 'path'
import fs from 'fs'
import { type RootLog } from '@pnpm/core-loggers'
import { depPathToFilename } from '@pnpm/dependency-path'
import { prepareEmpty } from '@pnpm/prepare'
import {
  addDependenciesToPackage,
  install,
} from '@pnpm/core'
import { fixtures } from '@pnpm/test-fixtures'
import { assertProject } from '@pnpm/assert-project'
import { sync as rimraf } from '@zkochan/rimraf'
import { isCI } from 'ci-info'
import sinon from 'sinon'
import { testDefaults } from '../utils'

const f = fixtures(__dirname)
const withGitProtocolDepFixture = f.find('with-git-protocol-dep')

test('from a github repo', async () => {
  const project = prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['kevva/is-negative'], testDefaults())

  project.has('is-negative')

  expect(manifest.dependencies).toStrictEqual({
    'is-negative': 'github:kevva/is-negative',
  })
})

test('from a github repo through URL', async () => {
  const project = prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['https://github.com/kevva/is-negative'], testDefaults())

  project.has('is-negative')

  expect(manifest.dependencies).toStrictEqual({ 'is-negative': 'github:kevva/is-negative' })
})

test('from a github repo with different name via named installation', async () => {
  const project = prepareEmpty()

  const reporter = sinon.spy()

  const manifest = await addDependenciesToPackage(
    {},
    ['say-hi@github:zkochan/hi#4cdebec76b7b9d1f6e219e06c42d92a6b8ea60cd'],
    testDefaults({ fastUnpack: false, reporter })
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

  const lockfile = project.readLockfile()
  expect(lockfile.importers['.'].dependencies).toStrictEqual({
    'say-hi': {
      specifier: 'github:zkochan/hi#4cdebec76b7b9d1f6e219e06c42d92a6b8ea60cd',
      version: 'https://codeload.github.com/zkochan/hi/tar.gz/4cdebec76b7b9d1f6e219e06c42d92a6b8ea60cd',
    },
  })

  project.isExecutable('.bin/hi')
  project.isExecutable('.bin/szia')
})

// This used to fail. Maybe won't be needed once api/install.ts gets refactored and covered with dedicated unit tests
test('from a github repo with different name', async () => {
  const project = prepareEmpty()

  const reporter = sinon.spy()

  const manifest = await install({
    dependencies: {
      'say-hi': 'github:zkochan/hi#4cdebec76b7b9d1f6e219e06c42d92a6b8ea60cd',
    },
  }, testDefaults({ fastUnpack: false, reporter }))

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

  const lockfile = project.readLockfile()
  expect(lockfile.importers['.'].dependencies).toStrictEqual({
    'say-hi': {
      specifier: 'github:zkochan/hi#4cdebec76b7b9d1f6e219e06c42d92a6b8ea60cd',
      version: 'https://codeload.github.com/zkochan/hi/tar.gz/4cdebec76b7b9d1f6e219e06c42d92a6b8ea60cd',
    },
  })

  project.isExecutable('.bin/hi')
  project.isExecutable('.bin/szia')
})

test('a subdependency is from a github repo with different name', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['@pnpm.e2e/has-aliased-git-dependency'], testDefaults({ fastUnpack: false }))

  const m = project.requireModule('@pnpm.e2e/has-aliased-git-dependency')

  expect(m).toEqual('Hi')

  const lockfile = project.readLockfile()
  expect(lockfile.snapshots['/@pnpm.e2e/has-aliased-git-dependency@1.0.0'].dependencies).toStrictEqual({
    '@pnpm.e2e/has-say-hi-peer': '1.0.0(https://codeload.github.com/zkochan/hi/tar.gz/4cdebec76b7b9d1f6e219e06c42d92a6b8ea60cd)',
    'say-hi': 'https://codeload.github.com/zkochan/hi/tar.gz/4cdebec76b7b9d1f6e219e06c42d92a6b8ea60cd',
  })

  project.isExecutable('@pnpm.e2e/has-aliased-git-dependency/node_modules/.bin/hi')
  project.isExecutable('@pnpm.e2e/has-aliased-git-dependency/node_modules/.bin/szia')

  expect(fs.existsSync(path.resolve(`node_modules/.pnpm/${depPathToFilename('@pnpm.e2e/has-say-hi-peer@1.0.0(https://codeload.github.com/zkochan/hi/tar.gz/4cdebec76b7b9d1f6e219e06c42d92a6b8ea60cd)')}/node_modules/@pnpm.e2e/has-say-hi-peer`))).toBeTruthy()
})

test('from a git repo', async () => {
  if (isCI) {
    console.log('not testing the SSH GIT access via CI')
    return
  }
  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['git+ssh://git@github.com/kevva/is-negative.git'], testDefaults())

  project.has('is-negative')
})

// This test is unstable due to dependency on third party registry
test.skip('from a non-github git repo', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['git+http://ikt.pm2.io/ikt.git#3325a3e39a502418dc2e2e4bf21529cbbde96228'], testDefaults())

  const m = project.requireModule('ikt')

  expect(m).toBeTruthy()

  const lockfile = project.readLockfile()

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

  const manifest = await addDependenciesToPackage({}, ['pnpm/for-testing.no-package-json'], testDefaults())

  project.has('for-testing.no-package-json')

  expect(manifest.dependencies).toStrictEqual({
    'for-testing.no-package-json': 'github:pnpm/for-testing.no-package-json',
  })
  fs.rmSync(path.join(project.dir(), 'node_modules'), {
    recursive: true, force: true,
  })
  fs.rmSync(path.join(project.dir(), 'pnpm-lock.yaml'))
  // if there is an unresolved promise, this test will hang until timeout.
  // e.g. thrown: "Exceeded timeout of 240000 ms for a test.
  await addDependenciesToPackage({}, ['pnpm/for-testing.no-package-json'], testDefaults())
  project.has('for-testing.no-package-json')
})

test.skip('from a github repo that needs to be built. isolated node linker is used', async () => {
  const project = prepareEmpty()

  const manifest = await addDependenciesToPackage({}, ['pnpm-e2e/prepare-script-works'], testDefaults({ ignoreScripts: true }, { ignoreScripts: true }))

  project.hasNot('@pnpm.e2e/prepare-script-works/prepare.txt')

  rimraf('node_modules')
  await install(manifest, testDefaults({ preferFrozenLockfile: false }))
  project.has('@pnpm.e2e/prepare-script-works/prepare.txt')

  rimraf('node_modules')
  await install(manifest, testDefaults({ frozenLockfile: true }))
  project.has('@pnpm.e2e/prepare-script-works/prepare.txt')

  rimraf('node_modules')
  await install(manifest, testDefaults({ frozenLockfile: true, ignoreScripts: true }, { ignoreScripts: true }))
  project.hasNot('@pnpm.e2e/prepare-script-works/prepare.txt')
})

test.skip('from a github repo that needs to be built. hoisted node linker is  used', async () => {
  const project = prepareEmpty()

  const manifest = await addDependenciesToPackage(
    {},
    ['pnpm-e2e/prepare-script-works'],
    testDefaults({ ignoreScripts: true, nodeLinker: 'hoisted' }, { ignoreScripts: true })
  )

  project.hasNot('@pnpm.e2e/prepare-script-works/prepare.txt')

  rimraf('node_modules')
  await install(manifest, testDefaults({ preferFrozenLockfile: false, nodeLinker: 'hoisted' }))
  project.has('@pnpm.e2e/prepare-script-works/prepare.txt')

  rimraf('node_modules')
  await install(manifest, testDefaults({ frozenLockfile: true, nodeLinker: 'hoisted' }))
  project.has('@pnpm.e2e/prepare-script-works/prepare.txt')

  rimraf('node_modules')
  await install(manifest, testDefaults({ frozenLockfile: true, ignoreScripts: true, nodeLinker: 'hoisted' }, { ignoreScripts: true }))
  project.hasNot('@pnpm.e2e/prepare-script-works/prepare.txt')
})

test('re-adding a git repo with a different tag', async () => {
  const project = prepareEmpty()
  let manifest = await addDependenciesToPackage({}, ['kevva/is-negative#1.0.0'], testDefaults())
  project.has('is-negative')
  expect(manifest.dependencies).toStrictEqual({
    'is-negative': 'github:kevva/is-negative#1.0.0',
  })
  expect(JSON.parse(fs.readFileSync('./node_modules/is-negative/package.json', 'utf8')).version).toBe('1.0.0')
  let lockfile = project.readLockfile()
  expect(lockfile.importers['.'].dependencies?.['is-negative']).toEqual({
    specifier: 'github:kevva/is-negative#1.0.0',
    version: 'https://codeload.github.com/kevva/is-negative/tar.gz/163360a8d3ae6bee9524541043197ff356f8ed99',
  })
  expect(lockfile.packages).toEqual(
    {
      'https://codeload.github.com/kevva/is-negative/tar.gz/163360a8d3ae6bee9524541043197ff356f8ed99': {
        resolution: { tarball: 'https://codeload.github.com/kevva/is-negative/tar.gz/163360a8d3ae6bee9524541043197ff356f8ed99' },
        name: 'is-negative',
        version: '1.0.0',
        engines: { node: '>=0.10.0' },
      },
    }
  )
  manifest = await addDependenciesToPackage(manifest, ['kevva/is-negative#1.0.1'], testDefaults())
  project.has('is-negative')
  expect(JSON.parse(fs.readFileSync('./node_modules/is-negative/package.json', 'utf8')).version).toBe('1.0.1')
  lockfile = project.readLockfile()
  expect(lockfile.importers['.'].dependencies?.['is-negative']).toEqual({
    specifier: 'github:kevva/is-negative#1.0.1',
    version: 'https://codeload.github.com/kevva/is-negative/tar.gz/9a89df745b2ec20ae7445d3d9853ceaeef5b0b72',
  })
  expect(lockfile.packages).toEqual(
    {
      'https://codeload.github.com/kevva/is-negative/tar.gz/9a89df745b2ec20ae7445d3d9853ceaeef5b0b72': {
        resolution: { tarball: 'https://codeload.github.com/kevva/is-negative/tar.gz/9a89df745b2ec20ae7445d3d9853ceaeef5b0b72' },
        name: 'is-negative',
        version: '1.0.1',
        engines: { node: '>=0.10.0' },
      },
    }
  )
})

test('should not update when adding unrelated dependency', async () => {
  process.chdir(withGitProtocolDepFixture)
  fs.rmSync('./node_modules', {
    recursive: true,
    force: true,
  })
  let manifest = JSON.parse(fs.readFileSync('./package.json', 'utf8'))
  await install(manifest, testDefaults({ preferFrozenLockfile: false, dir: withGitProtocolDepFixture, lockfileDir: withGitProtocolDepFixture }))

  expect(fs.readdirSync('./node_modules/.pnpm')).toContain('https+++codeload.github.com+kevva+is-negative+tar.gz+219c424611ff4a2af15f7deeff4f93c62558c43d') // cspell:disable-line

  manifest = await addDependenciesToPackage(manifest, ['is-number'], testDefaults({ preferFrozenLockfile: false, modulesCacheMaxAge: 0 }))

  expect(manifest.dependencies).toHaveProperty('is-number')
  expect(manifest.dependencies['is-negative']).toBe('github:kevva/is-negative#master')

  const project = assertProject(withGitProtocolDepFixture)
  project.has('is-number')
  expect(fs.existsSync('./node_modules/.pnpm/https+++codeload.github.com+kevva+is-negative+tar.gz+219c424611ff4a2af15f7deeff4f93c62558c43d')).toBe(true) // cspell:disable-line
  expect(project.readLockfile().importers['.'].dependencies).toEqual({
    'is-negative': {
      specifier: 'github:kevva/is-negative#master',
      version: 'https://codeload.github.com/kevva/is-negative/tar.gz/219c424611ff4a2af15f7deeff4f93c62558c43d',
    },
    'is-number': {
      specifier: '^7.0.0',
      version: '7.0.0',
    },
  })
})

test('git-hosted repository is not added to the store if it fails to be built', async () => {
  prepareEmpty()

  await expect(
    addDependenciesToPackage({}, ['pnpm-e2e/prepare-script-fails'], testDefaults())
  ).rejects.toThrow()

  await expect(
    addDependenciesToPackage({}, ['pnpm-e2e/prepare-script-fails'], testDefaults())
  ).rejects.toThrow()
})

test('from subdirectories of a git repo', async () => {
  const project = prepareEmpty()

  const manifest = await addDependenciesToPackage({}, [
    'github:RexSkz/test-git-subfolder-fetch#path:/packages/simple-react-app',
    'github:RexSkz/test-git-subfolder-fetch#path:/packages/simple-express-server',
  ], testDefaults())

  project.has('@my-namespace/simple-react-app')
  project.has('@my-namespace/simple-express-server')

  expect(manifest.dependencies).toStrictEqual({
    '@my-namespace/simple-express-server': 'github:RexSkz/test-git-subfolder-fetch#path:/packages/simple-express-server',
    '@my-namespace/simple-react-app': 'github:RexSkz/test-git-subfolder-fetch#path:/packages/simple-react-app',
  })
})
