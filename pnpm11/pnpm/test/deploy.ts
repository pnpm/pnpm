import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { preparePackages } from '@pnpm/prepare'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm } from './utils/index.js'

// `pacquet` is fetched from the real npm registry — registry-mock doesn't
// carry it (or its platform-specific binary sub-packages), so this test
// requires the public registry to be reachable. Matches the pattern in
// `pnpm/test/install/pacquet.ts`.
const PUBLIC_REGISTRY = '--config.registry=https://registry.npmjs.org/'
const PACQUET_VERSION = '0.2.2'

// Two installs against the public registry plus a deploy; raise the per-test
// timeout above jest's 5s default to allow for cold caches.
const PUBLIC_REGISTRY_TIMEOUT = 5 * 60 * 1000

test('deploy with a shared lockfile succeeds when pacquet is declared in configDependencies', async () => {
  preparePackages([
    { location: '.', package: { name: 'root', version: '0.0.0' } },
    {
      location: 'services/foo',
      package: {
        name: 'foo',
        version: '0.0.0',
        dependencies: { 'is-positive': '3.1.0' },
      },
    },
  ])

  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['services/*'],
    configDependencies: { pacquet: PACQUET_VERSION },
    injectWorkspacePackages: true,
  })

  await execPnpm([PUBLIC_REGISTRY, 'install'])

  const deployDir = path.resolve('services/foo/pnpm.out')
  await execPnpm([PUBLIC_REGISTRY, '--filter=foo', 'deploy', deployDir])

  expect(fs.existsSync(path.join(deployDir, 'node_modules/is-positive/package.json'))).toBe(true)
}, PUBLIC_REGISTRY_TIMEOUT)
