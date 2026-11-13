import { expect, test } from '@jest/globals'
import {
  preparePackages,
} from '@pnpm/prepare'
import { createTestIpcServer } from '@pnpm/test-ipc-server'
import type { ProjectManifest } from '@pnpm/types'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm } from './utils/index.js'

test.each([
  { message: '--filter should include devDependencies', filter: '--filter', expected: ['project-1', 'project-3', 'project-4'] },
  { message: '--filter-prod should not include devDependencies', filter: '--filter-prod', expected: ['project-1', 'project-3'] },
])('$message', async ({ filter, expected }) => {
  await using server = await createTestIpcServer()

  const projects: Array<ProjectManifest & { name: string }> = [
    {
      name: 'project-1',
      version: '1.0.0',
      dependencies: { 'project-2': 'workspace:*', 'project-3': 'workspace:*' },
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
      dependencies: { 'project-2': 'workspace:*' },
      scripts: {
        test: server.sendLineScript('project-3'),
      },
    },
    {
      name: 'project-4',
      version: '1.0.0',
      dependencies: {},
      devDependencies: { 'project-3': 'workspace:*' },
      scripts: {
        test: server.sendLineScript('project-4'),
      },
    },
  ]
  preparePackages(projects)

  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  await execPnpm(['install'])

  await execPnpm([filter, '...project-3', 'test'])

  expect(server.getLines().sort()).toEqual(expected)
})

test('combining --filter-prod with an exclusion --filter excludes target without selecting unrelated projects', async () => {
  await using server = await createTestIpcServer()

  preparePackages([
    {
      location: '.',
      package: {
        name: 'ws-root',
        private: true,
        scripts: { check: server.sendLineScript('ws-root') },
      },
    },
    {
      location: 'packages/app',
      package: {
        name: 'app',
        version: '1.0.0',
        dependencies: { 'lib-a': 'workspace:*' },
        devDependencies: { 'lib-dev': 'workspace:*' },
        scripts: { check: server.sendLineScript('app') },
      },
    },
    {
      location: 'packages/lib-a',
      package: {
        name: 'lib-a',
        version: '1.0.0',
        scripts: { check: server.sendLineScript('lib-a') },
      },
    },
    {
      location: 'packages/lib-dev',
      package: {
        name: 'lib-dev',
        version: '1.0.0',
        scripts: { check: server.sendLineScript('lib-dev') },
      },
    },
    {
      location: 'packages/unrelated',
      package: {
        name: 'unrelated',
        version: '1.0.0',
        scripts: { check: server.sendLineScript('unrelated') },
      },
    },
  ])

  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['packages/*'] })
  await execPnpm(['install'])

  await execPnpm(['--filter-prod', 'app...', '--filter', '!lib-a', 'run', 'check'])
  expect(server.getLines().sort()).toEqual(['app'])

  server.clear()
  await execPnpm(['--filter', 'app...', '--filter-prod', '!lib-a', 'run', 'check'])
  expect(server.getLines().sort()).toEqual(['app', 'lib-dev'])
})
