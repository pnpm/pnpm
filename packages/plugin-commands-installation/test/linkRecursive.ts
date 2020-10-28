import { LOCKFILE_VERSION } from '@pnpm/constants'
import { readProjects } from '@pnpm/filter-workspace-packages'
import { install, unlink } from '@pnpm/plugin-commands-installation'
import { preparePackages } from '@pnpm/prepare'
import { DEFAULT_OPTS } from './utils'
import path = require('path')
import exists = require('path-exists')

test('recursive linking/unlinking', async () => {
  const projects = preparePackages(undefined, [
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

  expect(projects['is-positive'].requireModule('is-negative')).toBeTruthy()
  expect(projects['project-1'].requireModule('is-positive/package.json').author).toBeFalsy()

  {
    const project1Lockfile = await projects['project-1'].readLockfile()
    expect(project1Lockfile.devDependencies['is-positive']).toBe('link:../is-positive')
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
  expect(await exists(path.resolve('node_modules', 'is-positive', 'index.js'))).toBeTruthy()

  {
    const project1Lockfile = await projects['project-1'].readLockfile()
    expect(project1Lockfile.lockfileVersion).toBe(LOCKFILE_VERSION)
    expect(project1Lockfile.devDependencies['is-positive']).toBe('1.0.0')
    expect(project1Lockfile.packages['/is-positive/1.0.0']).toBeTruthy()
  }

  const isPositiveLockfile = await projects['is-positive'].readLockfile()
  expect(isPositiveLockfile.lockfileVersion).toBe(LOCKFILE_VERSION)
})

test('recursive unlink specific package', async () => {
  const projects = preparePackages(undefined, [
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

  expect(projects['is-positive'].requireModule('is-negative')).toBeTruthy()
  expect(projects['project-1'].requireModule('is-positive/package.json').author).toBeFalsy()

  {
    const project1Lockfile = await projects['project-1'].readLockfile()
    expect(project1Lockfile.devDependencies['is-positive']).toBe('link:../is-positive')
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
  expect(await exists(path.resolve('node_modules', 'is-positive', 'index.js'))).toBeTruthy()

  {
    const project1Lockfile = await projects['project-1'].readLockfile()
    expect(project1Lockfile.lockfileVersion).toBe(LOCKFILE_VERSION)
    expect(project1Lockfile.devDependencies['is-positive']).toBe('1.0.0')
    expect(project1Lockfile.packages['/is-positive/1.0.0']).toBeTruthy()
  }

  const isPositiveLockfile = await projects['is-positive'].readLockfile()
  expect(isPositiveLockfile.lockfileVersion).toBe(LOCKFILE_VERSION)
})
