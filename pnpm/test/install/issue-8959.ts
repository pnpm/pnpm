import fs from 'fs'
import { preparePackages } from '@pnpm/prepare'
import { sync as writeYamlFile } from 'write-yaml-file'
import { execPnpm } from '../utils'

// Covers https://github.com/pnpm/pnpm/issues/8959
test('restores deleted modules dir of a workspace package', async () => {
  preparePackages([
    {
      location: '.',
      package: {
        name: 'root',
        version: '0.0.0',
        private: true,
      },
    },
    {
      location: 'packages/foo',
      package: {
        name: 'foo',
        version: '0.0.0',
        private: true,
        dependencies: {
          'is-positive': '1.0.0',
        },
      },
    },
  ])

  writeYamlFile('pnpm-workspace.yaml', { packages: ['packages/*'] })

  await execPnpm(['install'])
  expect(fs.readdirSync('node_modules')).toContain('.pnpm-workspace-state.json')
  expect(fs.readdirSync('packages/foo/node_modules')).toContain('is-positive')

  fs.rmSync('packages/foo/node_modules', { recursive: true })
  await execPnpm(['--reporter=append-only', 'install'])

  expect(fs.readdirSync('packages/foo/node_modules')).toContain('is-positive')
})
