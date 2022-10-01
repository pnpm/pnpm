import path from 'path'
import writeYamlFile from 'write-yaml-file'
import { execPnpm } from './utils'
import {
  preparePackages,
} from '@pnpm/prepare'

test.each([
  { message: '--filter should include devDependencies', filter: '--filter', expected: ['project-1', 'project-3', 'project-4'] },
  { message: '--filter-prod should not include devDependencies', filter: '--filter-prod', expected: ['project-1', 'project-3'] },
])('$message', async ({ filter, expected }) => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
      dependencies: { 'project-2': '1.0.0', 'project-3': '1.0.0' },
      devDependencies: { 'json-append': '1' },
      scripts: {
        test: 'node -e "process.stdout.write(\'project-1\')" | json-append ../output.json',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',
      dependencies: {},
      devDependencies: { 'json-append': '1' },
      scripts: {
        test: 'node -e "process.stdout.write(\'project-2\')" | json-append ../output.json',
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',
      dependencies: { 'project-2': '1.0.0' },
      devDependencies: { 'json-append': '1' },
      scripts: {
        test: 'node -e "process.stdout.write(\'project-3\')" | json-append ../output.json',
      },
    },
    {
      name: 'project-4',
      version: '1.0.0',
      dependencies: {},
      devDependencies: { 'json-append': '1', 'project-3': '1.0.0' },
      scripts: {
        test: 'node -e "process.stdout.write(\'project-4\')" | json-append ../output.json',
      },
    },
  ])

  await writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  await execPnpm(['install'])

  await execPnpm(['recursive', 'test', filter, '...project-3'])
  const { default: output } = await import(path.resolve('output.json'))
  expect(output.sort()).toStrictEqual(expected)
})