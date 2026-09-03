import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { assertStore } from '@pnpm/assert-store'
import { ABBREVIATED_META_DIR, WANTED_LOCKFILE } from '@pnpm/constants'
import {
  addDependenciesToPackage,
  install,
  mutateModules,
} from '@pnpm/installing.deps-installer'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import { addDistTag, REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'
import type { ProjectRootDir } from '@pnpm/types'
import { readYamlFileSync } from 'read-yaml-file'

import { testDefaults } from '../utils/index.js'

test('install with lockfileOnly = true', async () => {
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })
  const project = prepareEmpty()

  const opts = testDefaults({ lockfileOnly: true, rangeSpecStyle: 'patch' as const })
  const { updatedManifest: manifest } = await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-1-dep@100.0.0'], opts)
  const { cafsHasNot } = assertStore(opts.storeDir)

  cafsHasNot('@pnpm.e2e/pkg-with-1-dep', '100.0.0')
  expect(fs.existsSync(path.join(opts.cacheDir, `${ABBREVIATED_META_DIR}/localhost+${REGISTRY_MOCK_PORT}/@pnpm.e2e/pkg-with-1-dep.jsonl`))).toBeTruthy()
  cafsHasNot('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.1.0')
  expect(fs.existsSync(path.join(opts.cacheDir, `${ABBREVIATED_META_DIR}/localhost+${REGISTRY_MOCK_PORT}/@pnpm.e2e/dep-of-pkg-with-1-dep.jsonl`))).toBeTruthy()
  project.hasNot('@pnpm.e2e/pkg-with-1-dep')

  expect(manifest.dependencies!['@pnpm.e2e/pkg-with-1-dep']).toBe('100.0.0')

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

test('a partial workspace install with lockfileOnly does not create node_modules', async () => {
  preparePackages([
    {
      location: 'project-1',
      package: { name: 'project-1' },
    },
    {
      location: 'project-2',
      package: { name: 'project-2' },
    },
  ])
  const allProjects = [
    {
      buildIndex: 0,
      manifest: {
        name: 'project-1',
        dependencies: {
          'is-positive': '1.0.0',
        },
      },
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: {
        name: 'project-2',
        dependencies: {
          'is-negative': '1.0.0',
        },
      },
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
  ]

  await mutateModules([{
    mutation: 'install',
    rootDir: allProjects[0].rootDir,
  }], testDefaults({ allProjects, lockfileOnly: true }))

  expect(fs.existsSync(path.resolve('node_modules'))).toBe(false)
  expect(fs.existsSync(path.resolve('project-1/node_modules'))).toBe(false)
  expect(fs.existsSync(path.resolve('project-2/node_modules'))).toBe(false)
  const lockfile = readYamlFileSync(WANTED_LOCKFILE) as { importers: Record<string, unknown> }
  expect(Object.keys(lockfile.importers)).toStrictEqual(['project-1', 'project-2'])
})
