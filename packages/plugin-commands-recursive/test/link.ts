import { WANTED_LOCKFILE } from '@pnpm/constants'
import { recursive } from '@pnpm/plugin-commands-recursive'
import { preparePackages } from '@pnpm/prepare'
import path = require('path')
import exists = require('path-exists')
import test = require('tape')
import { DEFAULT_OPTS } from './utils'

test('recursive linking/unlinking', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      devDependencies: {
        'is-positive': '1.0.0',
      },
    },
    {
      name: 'is-positive',
      version: '1.0.0',

      dependencies: {
        'is-negative': '1.0.0',
      },
    },
  ])

  await recursive.handler(['install'], {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })

  t.ok(projects['is-positive'].requireModule('is-negative'))
  t.notOk(projects['project-1'].requireModule('is-positive/package.json').author, 'local package is linked')

  {
    const project1Lockfile = await projects['project-1'].readLockfile()
    t.equal(project1Lockfile.devDependencies['is-positive'], 'link:../is-positive')
  }

  await recursive.handler(['unlink'], {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })

  process.chdir('project-1')
  t.ok(await exists(path.resolve('node_modules', 'is-positive', 'index.js')), 'local package is unlinked')

  {
    const project1Lockfile = await projects['project-1'].readLockfile()
    t.equal(project1Lockfile.lockfileVersion, 5.1, `project-1 has correct lockfileVersion specified in ${WANTED_LOCKFILE}`)
    t.equal(project1Lockfile.devDependencies['is-positive'], '1.0.0')
    t.ok(project1Lockfile.packages['/is-positive/1.0.0'])
  }

  const isPositiveLockfile = await projects['is-positive'].readLockfile()
  t.equal(isPositiveLockfile.lockfileVersion, 5.1, `is-positive has correct lockfileVersion specified in ${WANTED_LOCKFILE}`)

  t.end()
})

test('recursive unlink specific package', async (t) => {
  const projects = preparePackages(t, [
    {
      name: 'project-1',
      version: '1.0.0',

      devDependencies: {
        'is-positive': '1.0.0',
      },
    },
    {
      name: 'is-positive',
      version: '1.0.0',

      dependencies: {
        'is-negative': '1.0.0',
      },
    },
  ])

  await recursive.handler(['install'], {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })

  t.ok(projects['is-positive'].requireModule('is-negative'))
  t.notOk(projects['project-1'].requireModule('is-positive/package.json').author, 'local package is linked')

  {
    const project1Lockfile = await projects['project-1'].readLockfile()
    t.equal(project1Lockfile.devDependencies['is-positive'], 'link:../is-positive')
  }

  await recursive.handler(['unlink', 'is-positive'], {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })

  process.chdir('project-1')
  t.ok(await exists(path.resolve('node_modules', 'is-positive', 'index.js')), 'local package is unlinked')

  {
    const project1Lockfile = await projects['project-1'].readLockfile()
    t.equal(project1Lockfile.lockfileVersion, 5.1, `project-1 has correct lockfileVersion specified in ${WANTED_LOCKFILE}`)
    t.equal(project1Lockfile.devDependencies['is-positive'], '1.0.0')
    t.ok(project1Lockfile.packages['/is-positive/1.0.0'])
  }

  const isPositiveLockfile = await projects['is-positive'].readLockfile()
  t.equal(isPositiveLockfile.lockfileVersion, 5.1, `is-positive has correct lockfileVersion specified in ${WANTED_LOCKFILE}`)

  t.end()
})
