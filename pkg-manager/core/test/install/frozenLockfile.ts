import path from 'path'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import {
  install,
  type MutatedProject,
  mutateModules,
} from '@pnpm/core'
import { type ProjectRootDir } from '@pnpm/types'
import sinon from 'sinon'
import { testDefaults } from '../utils'

test(`frozen-lockfile: installation fails if specs in package.json don't match the ones in ${WANTED_LOCKFILE}`, async () => {
  prepareEmpty()

  await install(
    {
      dependencies: {
        'is-positive': '1.0.0',
      },
    },
    testDefaults()
  )

  await expect(
    install({
      dependencies: {
        'is-positive': '^3.1.0',
      },
    }, testDefaults({ frozenLockfile: true }))
  ).rejects.toThrow(`Cannot install with "frozen-lockfile" because ${WANTED_LOCKFILE} is not up to date with ${path.join('<ROOT>', 'package.json')}`)
})

test(`frozen-lockfile+hoistPattern: installation fails if specs in package.json don't match the ones in ${WANTED_LOCKFILE}`, async () => {
  prepareEmpty()

  await install({
    dependencies: {
      'is-positive': '1.0.0',
    },
  }, testDefaults({ hoistPattern: '*' }))

  await expect(
    install({
      dependencies: {
        'is-positive': '^3.1.0',
      },
    }, testDefaults({ frozenLockfile: true, hoistPattern: '*' }))
  ).rejects.toThrow(`Cannot install with "frozen-lockfile" because ${WANTED_LOCKFILE} is not up to date with ${path.join('<ROOT>', 'package.json')}`)
})

test(`frozen-lockfile: fail on a shared ${WANTED_LOCKFILE} that does not satisfy one of the package.json files`, async () => {
  prepareEmpty()

  const projects: MutatedProject[] = [
    {
      mutation: 'install',
      rootDir: path.resolve('p1') as ProjectRootDir,
    },
    {
      mutation: 'install',
      rootDir: path.resolve('p2') as ProjectRootDir,
    },
  ]
  const project1 = {
    buildIndex: 0,
    manifest: {
      name: 'p1',

      dependencies: {
        'is-positive': '1.0.0',
      },
    },
    rootDir: path.resolve('p1') as ProjectRootDir,
  }
  const project2 = {
    buildIndex: 0,
    manifest: {
      name: 'p2',

      dependencies: {
        'is-negative': '1.0.0',
      },
    },
    rootDir: path.resolve('p2') as ProjectRootDir,
  }
  await mutateModules(projects, testDefaults({
    allProjects: [project1, project2],
  }))

  project1.manifest = {
    ...project1.manifest,
    dependencies: {
      'is-positive': '^3.1.0',
    },
  }

  await expect(
    mutateModules(projects, testDefaults({ frozenLockfile: true, allProjects: [project1, project2] }))
  ).rejects.toThrow(`Cannot install with "frozen-lockfile" because ${WANTED_LOCKFILE} is not up to date with ${path.join('<ROOT>', 'p1/package.json')}`)
})

test(`frozen-lockfile: should successfully install when ${WANTED_LOCKFILE} is available`, async () => {
  const project = prepareEmpty()

  const { updatedManifest: manifest } = await install({
    dependencies: {
      'is-positive': '^3.0.0',
    },
  }, testDefaults({ lockfileOnly: true }))

  project.hasNot('is-positive')

  await install(manifest, testDefaults({ frozenLockfile: true }))

  project.has('is-positive')
})

test(`frozen-lockfile: should fail if no ${WANTED_LOCKFILE} is present`, async () => {
  prepareEmpty()

  await expect(
    install({
      dependencies: {
        'is-positive': '^3.0.0',
      },
    }, testDefaults({ frozenLockfile: true }))
  ).rejects.toThrow(`Cannot install with "frozen-lockfile" because ${WANTED_LOCKFILE} is absent`)
})

test(`prefer-frozen-lockfile: should prefer headless installation when ${WANTED_LOCKFILE} satisfies package.json`, async () => {
  const project = prepareEmpty()

  const { updatedManifest: manifest } = await install({
    dependencies: {
      'is-positive': '^3.0.0',
    },
  }, testDefaults({ lockfileOnly: true }))

  project.hasNot('is-positive')

  const reporter = sinon.spy()
  await install(manifest, testDefaults({ reporter, preferFrozenLockfile: true }))

  expect(reporter.calledWithMatch({
    level: 'info',
    message: 'Lockfile is up to date, resolution step is skipped',
    name: 'pnpm',
  })).toBeTruthy()

  project.has('is-positive')
})

test(`prefer-frozen-lockfile: should not prefer headless installation when ${WANTED_LOCKFILE} does not satisfy package.json`, async () => {
  const project = prepareEmpty()

  await install({
    dependencies: {
      'is-positive': '^3.0.0',
    },
  }, testDefaults({ lockfileOnly: true }))

  project.hasNot('is-positive')

  const reporter = sinon.spy()
  await install({
    dependencies: {
      'is-negative': '1.0.0',
    },
  }, testDefaults({ reporter, preferFrozenLockfile: true }))

  expect(reporter.calledWithMatch({
    level: 'info',
    message: 'Lockfile is up to date, resolution step is skipped',
    name: 'pnpm',
  })).toBeFalsy()

  project.has('is-negative')
})

test(`prefer-frozen-lockfile: should not fail if no ${WANTED_LOCKFILE} is present and project has no deps`, async () => {
  prepareEmpty()

  await install({}, testDefaults({ preferFrozenLockfile: true }))
})

test(`frozen-lockfile: should not fail if no ${WANTED_LOCKFILE} is present and project has no deps`, async () => {
  prepareEmpty()

  await install({}, testDefaults({ frozenLockfile: true }))
})

test(`prefer-frozen-lockfile+hoistPattern: should prefer headless installation when ${WANTED_LOCKFILE} satisfies package.json`, async () => {
  const project = prepareEmpty()

  const { updatedManifest: manifest } = await install({
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
  }, testDefaults({ lockfileOnly: true }))

  project.hasNot('@pnpm.e2e/pkg-with-1-dep')

  const reporter = sinon.spy()
  await install(manifest, testDefaults({
    hoistPattern: '*',
    preferFrozenLockfile: true,
    reporter,
  }))

  expect(reporter.calledWithMatch({
    level: 'info',
    message: 'Lockfile is up to date, resolution step is skipped',
    name: 'pnpm',
  })).toBeTruthy()

  project.has('@pnpm.e2e/pkg-with-1-dep')
  project.has('.pnpm/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep')
})

test('prefer-frozen-lockfile: should prefer frozen-lockfile when package has linked dependency', async () => {
  const projects = preparePackages([
    {
      name: 'p1',

      dependencies: {
        p2: 'link:../p2',
      },
    },
    {
      name: 'p2',

      dependencies: {
        'is-negative': '1.0.0',
      },
    },
  ])

  const mutatedProjects: MutatedProject[] = [
    {
      mutation: 'install',
      rootDir: path.resolve('p1') as ProjectRootDir,
    },
    {
      mutation: 'install',
      rootDir: path.resolve('p2') as ProjectRootDir,
    },
  ]
  const allProjects = [
    {
      buildIndex: 0,
      manifest: {
        name: 'p1',

        dependencies: {
          p2: 'link:../p2',
        },
      },
      rootDir: path.resolve('p1') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: {
        name: 'p2',

        dependencies: {
          'is-negative': '1.0.0',
        },
      },
      rootDir: path.resolve('p2') as ProjectRootDir,
    },
  ]
  await mutateModules(mutatedProjects, testDefaults({ allProjects }))

  const reporter = sinon.spy()
  await mutateModules(mutatedProjects, testDefaults({
    allProjects,
    preferFrozenLockfile: true,
    reporter,
  }))

  expect(reporter.calledWithMatch({
    level: 'info',
    message: 'Lockfile is up to date, resolution step is skipped',
    name: 'pnpm',
  })).toBeTruthy()

  projects['p1'].has('p2')
  projects['p2'].has('is-negative')
})

test('frozen-lockfile: installation fails if the value of auto-install-peers changes', async () => {
  prepareEmpty()
  const manifest = {
    dependencies: {
      'is-positive': '^3.0.0',
    },
  }

  await install(manifest, testDefaults({ autoInstallPeers: true }))

  await expect(
    install(manifest, testDefaults({ frozenLockfile: true, autoInstallPeers: false }))
  ).rejects.toThrow('Cannot proceed with the frozen installation. The current "settings.autoInstallPeers" configuration doesn\'t match the value found in the lockfile')
})
