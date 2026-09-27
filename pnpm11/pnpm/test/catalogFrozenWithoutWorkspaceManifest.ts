import fs from 'node:fs'

import { expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm, execPnpmSync } from './utils/index.js'

test('a frozen install uses the catalogs recorded in the lockfile when there is no pnpm-workspace.yaml (#10551)', async () => {
  const project = prepareCatalogProject()
  await execPnpm(['install', '--lockfile-only'])
  fs.rmSync('pnpm-workspace.yaml')

  await execPnpm(['install', '--frozen-lockfile'])

  project.has('@pnpm.e2e/foo')
})

test('a frozen install fails when an empty pnpm-workspace.yaml drops the recorded catalogs', async () => {
  prepareCatalogProject()
  await execPnpm(['install', '--lockfile-only'])
  fs.writeFileSync('pnpm-workspace.yaml', '')

  const result = execPnpmSync(['install', '--frozen-lockfile'])

  expect(result.status).not.toBe(0)
  expect(result.stdout.toString()).toContain('ERR_PNPM_LOCKFILE_CONFIG_MISMATCH')
})

function prepareCatalogProject () {
  const project = prepare({
    name: 'catalog-frozen-without-workspace-manifest',
    version: '0.0.0',
    private: true,
    dependencies: {
      '@pnpm.e2e/foo': 'catalog:',
    },
  })
  writeYamlFileSync('pnpm-workspace.yaml', {
    catalog: {
      '@pnpm.e2e/foo': '100.0.0',
    },
  })
  return project
}
