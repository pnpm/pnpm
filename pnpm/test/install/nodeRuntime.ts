import fs from 'node:fs'

import { expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'
import { readYamlFileSync } from 'read-yaml-file'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm } from '../utils/index.js'

test('installing a CLI tool that requires a specific version of Node.js to be installed alongside it', async () => {
  prepare()
  fs.writeFileSync('pnpm-workspace.yaml', 'allowBuilds: { "@pnpm.e2e/cli-with-node-engine@1.0.0": true }', 'utf8')

  await execPnpm(['add', '@pnpm.e2e/cli-with-node-engine@1.0.0'])
  await execPnpm(['exec', 'cli-with-node-engine'])
  expect(fs.readFileSync('node-version', 'utf8')).toBe('v22.19.0')
})

test('a devEngines.runtime is never promoted into a catalog under catalogMode=strict', async () => {
  const project = prepare({
    devEngines: {
      runtime: {
        name: 'node',
        version: '24.0.0',
        onFail: 'download',
      },
    },
  })
  writeYamlFileSync('pnpm-workspace.yaml', { catalogMode: 'strict' })

  await execPnpm(['install'])

  // The runtime resolves and installs as usual.
  project.isExecutable('.bin/node')

  // It is not turned into a catalog entry...
  const workspaceManifest = readYamlFileSync<{ catalog?: unknown, catalogs?: unknown }>('pnpm-workspace.yaml')
  expect(workspaceManifest.catalog).toBeUndefined()
  expect(workspaceManifest.catalogs).toBeUndefined()

  // ...and stays in devEngines.runtime instead of leaking into devDependencies.
  const manifest = JSON.parse(fs.readFileSync('package.json', 'utf8'))
  expect(manifest.devEngines.runtime).toMatchObject({ name: 'node', version: '24.0.0' })
  expect(manifest.devDependencies?.node).toBeUndefined()

  const lockfile = project.readLockfile()
  expect(lockfile.importers['.'].devDependencies).toStrictEqual({
    node: {
      specifier: 'runtime:24.0.0',
      version: 'runtime:24.0.0',
    },
  })
})
