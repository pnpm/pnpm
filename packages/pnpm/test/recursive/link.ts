import { WANTED_LOCKFILE } from '@pnpm/constants'
import { preparePackages } from '@pnpm/prepare'
import path = require('path')
import exists = require('path-exists')
import tape = require('tape')
import promisifyTape from 'tape-promise'
import { execPnpm } from '../utils'

const test = promisifyTape(tape)

test('recursive linking/unlinking', async (t: tape.Test) => {
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

  await execPnpm('recursive', 'install')

  t.ok(projects['is-positive'].requireModule('is-negative'))
  t.notOk(projects['project-1'].requireModule('is-positive/package.json').author, 'local package is linked')

  {
    const project1Lockfile = await projects['project-1'].loadLockfile()
    t.equal(project1Lockfile.devDependencies['is-positive'], 'link:../is-positive')
  }

  await execPnpm('recursive', 'unlink')

  process.chdir('project-1')
  t.ok(await exists(path.resolve('node_modules', 'is-positive', 'index.js')), 'local package is unlinked')

  {
    const project1Lockfile = await projects['project-1'].loadLockfile()
    t.equal(project1Lockfile.lockfileVersion, 5, `project-1 has correct lockfileVersion specified in ${WANTED_LOCKFILE}`)
    t.equal(project1Lockfile.devDependencies['is-positive'], '1.0.0')
    t.ok(project1Lockfile.packages['/is-positive/1.0.0'])
  }

  const isPositiveLockfile = await projects['is-positive'].loadLockfile()
  t.equal(isPositiveLockfile.lockfileVersion, 5, `is-positive has correct lockfileVersion specified in ${WANTED_LOCKFILE}`)
})

test('recursive unlink specific package', async (t: tape.Test) => {
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

  await execPnpm('recursive', 'install')

  t.ok(projects['is-positive'].requireModule('is-negative'))
  t.notOk(projects['project-1'].requireModule('is-positive/package.json').author, 'local package is linked')

  {
    const project1Lockfile = await projects['project-1'].loadLockfile()
    t.equal(project1Lockfile.devDependencies['is-positive'], 'link:../is-positive')
  }

  await execPnpm('recursive', 'unlink', 'is-positive')

  process.chdir('project-1')
  t.ok(await exists(path.resolve('node_modules', 'is-positive', 'index.js')), 'local package is unlinked')

  {
    const project1Lockfile = await projects['project-1'].loadLockfile()
    t.equal(project1Lockfile.lockfileVersion, 5, `project-1 has correct lockfileVersion specified in ${WANTED_LOCKFILE}`)
    t.equal(project1Lockfile.devDependencies['is-positive'], '1.0.0')
    t.ok(project1Lockfile.packages['/is-positive/1.0.0'])
  }

  const isPositiveLockfile = await projects['is-positive'].loadLockfile()
  t.equal(isPositiveLockfile.lockfileVersion, 5, `is-positive has correct lockfileVersion specified in ${WANTED_LOCKFILE}`)
})
