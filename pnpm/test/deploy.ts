import fs from 'fs'
import path from 'path'
import { sync as writeYamlFile } from 'write-yaml-file'
import { assertProject } from '@pnpm/assert-project'
import { preparePackages } from '@pnpm/prepare'
import { execPnpm } from './utils'

test('deploy with node-linker=hoisted', async () => {
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
      files: ['index.js'],
      dependencies: {
        'project-3': 'workspace:*',
        'is-odd': '1.0.0',
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

  writeYamlFile('pnpm-workspace.yaml', {
    packages: ['project-1', 'project-2', 'project-3'],
  })

  for (const name of ['project-1', 'project-2', 'project-3']) {
    fs.writeFileSync(`${name}/test.js`, '', 'utf8')
    fs.writeFileSync(`${name}/index.js`, '', 'utf8')
  }

  const config = [
    '--config.node-linker=hoisted',
    `--config.store-dir=${path.resolve('store')}`,
    `--config.cache-dir=${path.resolve('cache')}`,
  ]

  await execPnpm([...config, 'install'])

  await execPnpm([...config, '--filter=project-1', '--prod', 'deploy', 'deploy'])

  const project = assertProject(path.resolve('deploy'))
  project.has('project-2') // project-1 depends on project-2
  project.has('project-3') // project-1 depends on project-2 which depends on project-3
  project.has('is-positive') // project-1 depends on is-positive
  project.hasNot('is-negative')
  expect(fs.existsSync('deploy/index.js')).toBeTruthy()
  expect(fs.existsSync('deploy/test.js')).toBeFalsy()
  expect(fs.existsSync('deploy/node_modules/.modules.yaml')).toBeTruthy()
  expect(fs.existsSync('deploy/node_modules/project-2/index.js')).toBeTruthy()
  expect(fs.existsSync('deploy/node_modules/project-2/test.js')).toBeFalsy()
  expect(fs.existsSync('deploy/node_modules/project-3/index.js')).toBeTruthy()
  expect(fs.existsSync('deploy/node_modules/project-3/test.js')).toBeFalsy()
  expect(fs.existsSync('pnpm-lock.yaml')).toBeFalsy() // no changes to the lockfile are written
})
