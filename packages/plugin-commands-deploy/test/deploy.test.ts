import fs from 'fs'
import path from 'path'
import { deploy } from '@pnpm/plugin-commands-deploy'
import assertProject from '@pnpm/assert-project'
import { preparePackages } from '@pnpm/prepare'
import { readProjects } from '@pnpm/filter-workspace-packages'
import { DEFAULT_OPTS } from './utils'

test('deploy', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
      files: ['index.js'],
      dependencies: {
        'project-2': 'workspace:*',
        'is-positive': '1.0.0',
      },
      devDependencies: {
        'project-3': 'workspace:*',
        'is-negative': '1.0.0',
      },
    },
    {
      name: 'project-2',
      version: '2.0.0',
      dependencies: {
        'project-3': 'workspace:*',
        'is-odd': '1.0.0',
      },
    },
    {
      name: 'project-3',
      version: '2.0.0',
      dependencies: {
        'project-3': 'workspace:*',
        'is-odd': '1.0.0',
      },
    },
  ])

  fs.writeFileSync('project-1/test.js', '', 'utf8')
  fs.writeFileSync('project-1/index.js', '', 'utf8')

  const { allProjects, selectedProjectsGraph } = await readProjects(process.cwd(), [{ namePattern: 'project-1' }])

  await deploy.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    dev: false,
    production: true,
    recursive: true,
    selectedProjectsGraph,
    sharedWorkspaceLockfile: true,
    lockfileDir: process.cwd(),
    workspaceDir: process.cwd(),
  }, ['deploy'])

  const project = assertProject(path.resolve('deploy'))
  await project.has('project-2')
  await project.has('is-positive')
  await project.hasNot('project-3')
  await project.hasNot('is-negative')
  expect(fs.existsSync('deploy/index.js')).toBeTruthy()
  expect(fs.existsSync('deploy/test.js')).toBeFalsy()
  expect(fs.existsSync('pnpm-lock.yaml')).toBeFalsy() // no changes to the lockfile are written
})
