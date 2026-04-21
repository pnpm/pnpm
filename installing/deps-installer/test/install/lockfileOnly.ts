import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { assertStore } from '@pnpm/assert-store'
import { ABBREVIATED_META_DIR } from '@pnpm/constants'
import {
  addDependenciesToPackage,
  install,
} from '@pnpm/installing.deps-installer'
import { prepareEmpty } from '@pnpm/prepare'
import { addDistTag, REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'

import { testDefaults } from '../utils/index.js'

test('install with lockfileOnly = true', async () => {
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })
  const project = prepareEmpty()

  const opts = testDefaults({ lockfileOnly: true, pinnedVersion: 'patch' as const })
  const { updatedManifest: manifest } = await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-1-dep@100.0.0'], opts)
  const { cafsHasNot } = assertStore(opts.storeDir)

  cafsHasNot('@pnpm.e2e/pkg-with-1-dep', '100.0.0')
  expect(fs.existsSync(path.join(opts.cacheDir, `${ABBREVIATED_META_DIR}/localhost+${REGISTRY_MOCK_PORT}/@pnpm.e2e/pkg-with-1-dep.jsonl`))).toBeTruthy()
  cafsHasNot('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.1.0')
  expect(fs.existsSync(path.join(opts.cacheDir, `${ABBREVIATED_META_DIR}/localhost+${REGISTRY_MOCK_PORT}/@pnpm.e2e/dep-of-pkg-with-1-dep.jsonl`))).toBeTruthy()
  project.hasNot('@pnpm.e2e/pkg-with-1-dep')

  expect(manifest.dependencies!['@pnpm.e2e/pkg-with-1-dep']).toBeTruthy()

  const lockfile = project.readLockfile()
  expect(lockfile.importers['.'].dependencies?.['@pnpm.e2e/pkg-with-1-dep']).toBeTruthy()
  expect(lockfile.packages['@pnpm.e2e/pkg-with-1-dep@100.0.0']).toBeTruthy()

  const currentLockfile = project.readCurrentLockfile()
  expect(currentLockfile).toBeFalsy()

  // console.log(`doing repeat install when ${WANTED_LOCKFILE} is available already`)
  await install(manifest, opts)

  cafsHasNot('@pnpm.e2e/pkg-with-1-dep', '100.0.0')
  expect(fs.existsSync(path.join(opts.cacheDir, `${ABBREVIATED_META_DIR}/localhost+${REGISTRY_MOCK_PORT}/@pnpm.e2e/pkg-with-1-dep.jsonl`))).toBeTruthy()
  cafsHasNot('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.1.0')
  expect(fs.existsSync(path.join(opts.cacheDir, `${ABBREVIATED_META_DIR}/localhost+${REGISTRY_MOCK_PORT}/@pnpm.e2e/dep-of-pkg-with-1-dep.jsonl`))).toBeTruthy()
  project.hasNot('@pnpm.e2e/pkg-with-1-dep')

  expect(project.readCurrentLockfile()).toBeFalsy()
})

test('do not update the lockfile when lockfileOnly and frozenLockfile are both used', async () => {
  prepareEmpty()
  await addDependenciesToPackage({}, ['is-positive@1.0.0'], testDefaults({
    lockfileOnly: true,
  }))
  await expect(install({
    dependencies: {
      'is-positive': '2.0.0',
    },
  }, testDefaults({
    lockfileOnly: true,
    frozenLockfile: true,
  }))).rejects.toThrow(/is not up to date/)
})
