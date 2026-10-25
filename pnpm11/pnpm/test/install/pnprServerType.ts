import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import type { LockfileFile } from '@pnpm/lockfile.types'
import { prepare } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'
import { readYamlFileSync } from 'read-yaml-file'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm } from '../utils/index.js'

const REGISTRY = `http://localhost:${REGISTRY_MOCK_PORT}/npm/`

test('serverType pnpr leaves the tarball URLs of the registry out of the lockfile', async () => {
  const project = prepare({
    dependencies: {
      '@pnpm.e2e/foo': '100.0.0',
      'is-positive': '1.0.0',
    },
  })
  const workspaceManifest = {
    registries: { [REGISTRY]: { serverType: 'pnpr' } },
  }
  writeYamlFileSync('pnpm-workspace.yaml', workspaceManifest)

  await execPnpm(['install'])

  expect(fs.readFileSync(WANTED_LOCKFILE, 'utf8')).not.toContain('tarball:')

  fs.rmSync('node_modules', { recursive: true })
  fs.rmSync(path.resolve('../store'), { recursive: true })
  await execPnpm(['install', '--frozen-lockfile'])

  project.has('@pnpm.e2e/foo')
  project.has('is-positive')

  writeYamlFileSync('pnpm-workspace.yaml', {
    ...workspaceManifest,
    overrides: { '@pnpm.e2e/foo': '100.1.0' },
  })
  await execPnpm(['install'])

  expect(Object.keys(readYamlFileSync<LockfileFile>(WANTED_LOCKFILE).packages ?? {})).toContain('@pnpm.e2e/foo@100.1.0')
  expect(fs.readFileSync(WANTED_LOCKFILE, 'utf8')).not.toContain('tarball:')
})
