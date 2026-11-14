import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { preparePackages } from '@pnpm/prepare'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm, execPnpmSync } from './utils/index.js'

test('peers inspects per-project lockfiles when sharedWorkspaceLockfile is false', async () => {
  preparePackages([
    {
      location: 'packages/app',
      package: {
        name: 'app',
        version: '1.0.0',
        dependencies: { 'peer-consumer': 'file:../../peer-consumer' },
      },
    },
    {
      location: 'packages/clean',
      package: {
        name: 'clean',
        version: '1.0.0',
      },
    },
  ])
  fs.mkdirSync('peer-consumer', { recursive: true })
  fs.writeFileSync(path.join('peer-consumer', 'package.json'), JSON.stringify({
    name: 'peer-consumer',
    version: '1.0.0',
    peerDependencies: { 'missing-peer': '^1.0.0' },
  }))
  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['packages/*'],
    autoInstallPeers: false,
    sharedWorkspaceLockfile: false,
    strictPeerDependencies: false,
  })
  await execPnpm(['install'])

  const recursive = execPnpmSync(['-r', 'peers'])
  expect(recursive.status).toBe(1)
  expect(recursive.stdout.toString()).toContain('missing-peer')

  const rootJson = execPnpmSync(['peers', 'check', '--json'])
  expect(rootJson.status).toBe(1)
  const parsedRoot = JSON.parse(rootJson.stdout.toString())
  expect(Object.keys(parsedRoot)).toEqual(['packages/app', 'packages/clean'])
  expect(parsedRoot['packages/app'].missing['missing-peer']).toHaveLength(1)
  expect(parsedRoot['packages/clean'].missing).toEqual({})

  const subprojectJson = execPnpmSync(['--dir', 'packages/app', 'peers', '--json'])
  expect(subprojectJson.status).toBe(1)
  const parsedSubproject = JSON.parse(subprojectJson.stdout.toString())
  expect(Object.keys(parsedSubproject)).toEqual(['.'])
  expect(parsedSubproject['.'].missing['missing-peer']).toHaveLength(1)

  const cleanFiltered = execPnpmSync(['--filter', 'clean', 'peers'], { expectSuccess: true })
  expect(cleanFiltered.stdout.toString()).toContain('No peer dependency issues found')
})
