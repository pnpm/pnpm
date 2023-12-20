import writeYamlFile from 'write-yaml-file'
import { execPnpm } from './utils'
import {
  preparePackages,
} from '@pnpm/prepare'
import { createTestIpcServer } from '@pnpm/test-ipc-server'
import { type ProjectManifest } from '@pnpm/types'

test.each([
  { message: '--filter should include devDependencies', filter: '--filter', expected: ['project-1', 'project-3', 'project-4'] },
  { message: '--filter-prod should not include devDependencies', filter: '--filter-prod', expected: ['project-1', 'project-3'] },
])('$message', async ({ filter, expected }) => {
  await using server = await createTestIpcServer()

  const projects: Array<ProjectManifest & { name: string }> = [
    {
      name: 'project-1',
      version: '1.0.0',
      dependencies: { 'project-2': '1.0.0', 'project-3': '1.0.0' },
      scripts: {
        test: server.sendLineScript('project-1'),
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',
      dependencies: {},
      scripts: {
        test: server.sendLineScript('project-2'),
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',
      dependencies: { 'project-2': '1.0.0' },
      scripts: {
        test: server.sendLineScript('project-3'),
      },
    },
    {
      name: 'project-4',
      version: '1.0.0',
      dependencies: {},
      devDependencies: { 'project-3': '1.0.0' },
      scripts: {
        test: server.sendLineScript('project-4'),
      },
    },
  ]
  preparePackages(projects)

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  await execPnpm(['install'])

  await execPnpm(['recursive', 'test', filter, '...project-3'])

  expect(server.getLines().sort()).toEqual(expected)
})
