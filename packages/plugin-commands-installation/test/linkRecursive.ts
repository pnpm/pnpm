import { LOCKFILE_VERSION, WANTED_LOCKFILE } from '@pnpm/constants'
import { readProjects } from '@pnpm/filter-workspace-packages'
import { install, link, unlink } from '@pnpm/plugin-commands-installation'
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

  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [])
  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  t.ok(projects['is-positive'].requireModule('is-negative'))
  t.notOk(projects['project-1'].requireModule('is-positive/package.json').author, 'local package is linked')

  {
    const project1Lockfile = await projects['project-1'].readLockfile()
    t.equal(project1Lockfile.devDependencies['is-positive'], 'link:../is-positive')
  }

  await unlink.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, [])

  process.chdir('project-1')
  t.ok(await exists(path.resolve('node_modules', 'is-positive', 'index.js')), 'local package is unlinked')

  {
    const project1Lockfile = await projects['project-1'].readLockfile()
    t.equal(project1Lockfile.lockfileVersion, LOCKFILE_VERSION, `project-1 has correct lockfileVersion specified in ${WANTED_LOCKFILE}`)
    t.equal(project1Lockfile.devDependencies['is-positive'], '1.0.0')
    t.ok(project1Lockfile.packages['/is-positive/1.0.0'])
  }

  const isPositiveLockfile = await projects['is-positive'].readLockfile()
  t.equal(isPositiveLockfile.lockfileVersion, LOCKFILE_VERSION, `is-positive has correct lockfileVersion specified in ${WANTED_LOCKFILE}`)

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

  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [])
  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  t.ok(projects['is-positive'].requireModule('is-negative'))
  t.notOk(projects['project-1'].requireModule('is-positive/package.json').author, 'local package is linked')

  {
    const project1Lockfile = await projects['project-1'].readLockfile()
    t.equal(project1Lockfile.devDependencies['is-positive'], 'link:../is-positive')
  }

  await unlink.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }, ['is-positive'])

  process.chdir('project-1')
  t.ok(await exists(path.resolve('node_modules', 'is-positive', 'index.js')), 'local package is unlinked')

  {
    const project1Lockfile = await projects['project-1'].readLockfile()
    t.equal(project1Lockfile.lockfileVersion, LOCKFILE_VERSION, `project-1 has correct lockfileVersion specified in ${WANTED_LOCKFILE}`)
    t.equal(project1Lockfile.devDependencies['is-positive'], '1.0.0')
    t.ok(project1Lockfile.packages['/is-positive/1.0.0'])
  }

  const isPositiveLockfile = await projects['is-positive'].readLockfile()
  t.equal(isPositiveLockfile.lockfileVersion, LOCKFILE_VERSION, `is-positive has correct lockfileVersion specified in ${WANTED_LOCKFILE}`)

  t.end()
})
