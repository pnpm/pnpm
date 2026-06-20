import { expect, test } from '@jest/globals'
import { preparePackages } from '@pnpm/prepare'
import { loadJsonFileSync } from 'load-json-file'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm } from './utils/index.js'

test('version --recursive bumps every workspace package', async () => {
  preparePackages([
    { name: 'project-1', version: '1.0.0' },
    { name: 'project-2', version: '2.3.0' },
  ])
  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['project-*'] })

  await execPnpm(['version', '--recursive', '--no-git-checks', 'minor'])

  expect(loadJsonFileSync<{ version: string }>('project-1/package.json').version).toBe('1.1.0')
  expect(loadJsonFileSync<{ version: string }>('project-2/package.json').version).toBe('2.4.0')
})

test('version --recursive --filter bumps only the selected package', async () => {
  preparePackages([
    { name: 'project-1', version: '1.0.0' },
    { name: 'project-2', version: '2.3.0' },
  ])
  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['project-*'] })

  await execPnpm(['version', '--recursive', '--filter', 'project-2', '--no-git-checks', 'patch'])

  expect(loadJsonFileSync<{ version: string }>('project-1/package.json').version).toBe('1.0.0')
  expect(loadJsonFileSync<{ version: string }>('project-2/package.json').version).toBe('2.3.1')
})
