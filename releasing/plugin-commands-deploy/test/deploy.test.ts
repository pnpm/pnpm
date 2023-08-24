import fs from 'fs'
import path from 'path'
import { deploy } from '@pnpm/plugin-commands-deploy'
import { assertProject } from '@pnpm/assert-project'
import { preparePackages } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { readProjects } from '@pnpm/filter-workspace-packages'
import crossSpawn from 'cross-spawn'
import { sync as loadJsonFile } from 'load-json-file'
import writeYamlFile from 'write-yaml-file'
import { DEFAULT_OPTS } from './utils'

const pnpmBin = path.join(__dirname, '../../../pnpm/bin/pnpm.cjs')

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
      main: 'local-file-1.js',
      publishConfig: {
        main: 'publish-file-1.js',
      },
    },
    {
      name: 'project-2',
      version: '2.0.0',
      files: ['index.js'],
      dependencies: {
        'project-3': 'workspace:*',
        'is-odd': '1.0.0',
      },
      main: 'local-file-2.js',
      publishConfig: {
        main: 'publish-file-2.js',
      },
    },
    {
      name: 'project-3',
      version: '2.0.0',
      files: ['index.js'],
      dependencies: {
        'project-3': 'workspace:*',
        'is-odd': '1.0.0',
      },
    },
  ])

  ; ['project-1', 'project-2', 'project-3'].forEach(name => {
    fs.writeFileSync(`${name}/test.js`, '', 'utf8')
    fs.writeFileSync(`${name}/index.js`, '', 'utf8')
  })

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['*'] })
  crossSpawn.sync(pnpmBin, ['install', '--ignore-scripts', '--store-dir=../store', `--registry=http://localhost:${REGISTRY_MOCK_PORT}`])
  fs.rmSync('pnpm-lock.yaml')

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
  expect(fs.existsSync('deploy/node_modules/.pnpm/file+project-2/node_modules/project-2/index.js')).toBeTruthy()
  expect(fs.existsSync('deploy/node_modules/.pnpm/file+project-2/node_modules/project-2/test.js')).toBeFalsy()
  expect(fs.existsSync('deploy/node_modules/.pnpm/file+project-3/node_modules/project-3/index.js')).toBeTruthy()
  expect(fs.existsSync('deploy/node_modules/.pnpm/file+project-3/node_modules/project-3/test.js')).toBeFalsy()
  expect(fs.existsSync('pnpm-lock.yaml')).toBeFalsy() // no changes to the lockfile are written
  const project1Manifest = loadJsonFile('deploy/package.json')
  expect(project1Manifest).toMatchObject({
    name: 'project-1',
    main: 'publish-file-1.js',
    dependencies: {
      'is-positive': '1.0.0',
      'project-2': '2.0.0',
    },
  })
  expect(project1Manifest).not.toHaveProperty('publishConfig')
  const project2Manifest = loadJsonFile('deploy/node_modules/.pnpm/file+project-2/node_modules/project-2/package.json')
  expect(project2Manifest).toMatchObject({
    name: 'project-2',
    main: 'publish-file-2.js',
    dependencies: {
      'project-3': '2.0.0',
      'is-odd': '1.0.0',
    },
  })
  expect(project2Manifest).not.toHaveProperty('publishConfig')
})

test('deploy with dedupePeerDependents=true ignores the value of dedupePeerDependents', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
      dependencies: {
        'is-positive': '1.0.0',
      },
    },
    {
      location: './sub-dir/project-2',
      package: {
        name: 'project-2',
        version: '2.0.0',
        dependencies: {
          'is-odd': '1.0.0',
        },
      },
    },
    {
      name: 'project-3',
      version: '2.0.0',
      dependencies: {
        'is-number': '1.0.0',
      },
    },
  ])

  const { allProjects, selectedProjectsGraph, allProjectsGraph } = await readProjects(process.cwd(), [{ namePattern: 'project-1' }])

  await deploy.handler({
    ...DEFAULT_OPTS,
    allProjects,
    allProjectsGraph,
    dir: process.cwd(),
    dev: false,
    production: true,
    recursive: true,
    selectedProjectsGraph,
    sharedWorkspaceLockfile: true,
    lockfileDir: process.cwd(),
    workspaceDir: process.cwd(),
    dedupePeerDependents: true, // This is ignored by deploy
  }, ['deploy'])
  const project = assertProject(path.resolve('deploy'))
  await project.has('is-positive')
  expect(fs.existsSync('sub-dir/deploy')).toBe(false)
})
