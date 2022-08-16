import path from 'path'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import {
  install,
  MutatedProject,
  mutateModules,
} from '@pnpm/core'
import sinon from 'sinon'
import { testDefaults } from '../utils'

test(`frozen-lockfile: installation fails if specs in package.json don't match the ones in ${WANTED_LOCKFILE}`, async () => {
  prepareEmpty()

  await install(
    {
      dependencies: {
        'is-positive': '^3.0.0',
      },
    },
    await testDefaults()
  )

  await expect(
    install({
      dependencies: {
        'is-positive': '^3.1.0',
      },
    }, await testDefaults({ frozenLockfile: true }))
  ).rejects.toThrow(`Cannot install with "frozen-lockfile" because ${WANTED_LOCKFILE} is not up to date with package.json`)
})

test(`frozen-lockfile+hoistPattern: installation fails if specs in package.json don't match the ones in ${WANTED_LOCKFILE}`, async () => {
  prepareEmpty()

  await install({
    dependencies: {
      'is-positive': '^3.0.0',
    },
  }, await testDefaults({ hoistPattern: '*' }))

  await expect(
    install({
      dependencies: {
        'is-positive': '^3.1.0',
      },
    }, await testDefaults({ frozenLockfile: true, hoistPattern: '*' }))
  ).rejects.toThrow(`Cannot install with "frozen-lockfile" because ${WANTED_LOCKFILE} is not up to date with package.json`)
})

test(`frozen-lockfile: fail on a shared ${WANTED_LOCKFILE} that does not satisfy one of the package.json files`, async () => {
  prepareEmpty()

  const projects: MutatedProject[] = [
    {
      buildIndex: 0,
      manifest: {
        name: 'p1',

        dependencies: {
          'is-positive': '^3.0.0',
        },
      },
      mutation: 'install',
      rootDir: path.resolve('p1'),
    },
    {
      buildIndex: 0,
      manifest: {
        name: 'p2',

        dependencies: {
          'is-negative': '1.0.0',
        },
      },
      mutation: 'install',
      rootDir: path.resolve('p2'),
    },
  ]
  await mutateModules(projects, await testDefaults())

  projects[0].manifest = {
    dependencies: {
      'is-positive': '^3.1.0',
    },
  }

  await expect(
    mutateModules(projects, await testDefaults({ frozenLockfile: true }))
  ).rejects.toThrow(`Cannot install with "frozen-lockfile" because ${WANTED_LOCKFILE} is not up to date with p1${path.sep}package.json`)
})

test(`frozen-lockfile: should successfully install when ${WANTED_LOCKFILE} is available`, async () => {
  const project = prepareEmpty()

  const manifest = await install({
    dependencies: {
      'is-positive': '^3.0.0',
    },
  }, await testDefaults({ lockfileOnly: true }))

  await project.hasNot('is-positive')

  await install(manifest, await testDefaults({ frozenLockfile: true }))

  await project.has('is-positive')
})

test(`frozen-lockfile: should fail if no ${WANTED_LOCKFILE} is present`, async () => {
  prepareEmpty()

  await expect(
    install({
      dependencies: {
        'is-positive': '^3.0.0',
      },
    }, await testDefaults({ frozenLockfile: true }))
  ).rejects.toThrow(`Headless installation requires a ${WANTED_LOCKFILE} file`)
})

test(`prefer-frozen-lockfile: should prefer headless installation when ${WANTED_LOCKFILE} satisfies package.json`, async () => {
  const project = prepareEmpty()

  const manifest = await install({
    dependencies: {
      'is-positive': '^3.0.0',
    },
  }, await testDefaults({ lockfileOnly: true }))

  await project.hasNot('is-positive')

  const reporter = sinon.spy()
  await install(manifest, await testDefaults({ reporter, preferFrozenLockfile: true }))

  expect(reporter.calledWithMatch({
    level: 'info',
    message: 'Lockfile is up to date, resolution step is skipped',
    name: 'pnpm',
  })).toBeTruthy()

  await project.has('is-positive')
})

test(`prefer-frozen-lockfile: should not prefer headless installation when ${WANTED_LOCKFILE} does not satisfy package.json`, async () => {
  const project = prepareEmpty()

  await install({
    dependencies: {
      'is-positive': '^3.0.0',
    },
  }, await testDefaults({ lockfileOnly: true }))

  await project.hasNot('is-positive')

  const reporter = sinon.spy()
  await install({
    dependencies: {
      'is-negative': '1.0.0',
    },
  }, await testDefaults({ reporter, preferFrozenLockfile: true }))

  expect(reporter.calledWithMatch({
    level: 'info',
    message: 'Lockfile is up to date, resolution step is skipped',
    name: 'pnpm',
  })).toBeFalsy()

  await project.has('is-negative')
})

test(`prefer-frozen-lockfile: should not fail if no ${WANTED_LOCKFILE} is present and project has no deps`, async () => {
  prepareEmpty()

  await install({}, await testDefaults({ preferFrozenLockfile: true }))
})

test(`frozen-lockfile: should not fail if no ${WANTED_LOCKFILE} is present and project has no deps`, async () => {
  prepareEmpty()

  await install({}, await testDefaults({ frozenLockfile: true }))
})

test(`prefer-frozen-lockfile+hoistPattern: should prefer headless installation when ${WANTED_LOCKFILE} satisfies package.json`, async () => {
  const project = prepareEmpty()

  const manifest = await install({
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
  }, await testDefaults({ lockfileOnly: true }))

  await project.hasNot('@pnpm.e2e/pkg-with-1-dep')

  const reporter = sinon.spy()
  await install(manifest, await testDefaults({
    hoistPattern: '*',
    preferFrozenLockfile: true,
    reporter,
  }))

  expect(reporter.calledWithMatch({
    level: 'info',
    message: 'Lockfile is up to date, resolution step is skipped',
    name: 'pnpm',
  })).toBeTruthy()

  await project.has('@pnpm.e2e/pkg-with-1-dep')
  await project.has('.pnpm/node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep')
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
      buildIndex: 0,
      manifest: {
        name: 'p1',

        dependencies: {
          p2: 'link:../p2',
        },
      },
      mutation: 'install',
      rootDir: path.resolve('p1'),
    },
    {
      buildIndex: 0,
      manifest: {
        name: 'p2',

        dependencies: {
          'is-negative': '1.0.0',
        },
      },
      mutation: 'install',
      rootDir: path.resolve('p2'),
    },
  ]
  await mutateModules(mutatedProjects, await testDefaults())

  const reporter = sinon.spy()
  await mutateModules(mutatedProjects, await testDefaults({
    preferFrozenLockfile: true,
    reporter,
  }))

  expect(reporter.calledWithMatch({
    level: 'info',
    message: 'Lockfile is up to date, resolution step is skipped',
    name: 'pnpm',
  })).toBeTruthy()

  await projects['p1'].has('p2')
  await projects['p2'].has('is-negative')
})
