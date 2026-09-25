/// <reference path="../../../__typings__/index.d.ts" />
import fs from 'node:fs'
import path from 'node:path'

import { beforeEach, expect, jest, test } from '@jest/globals'
import { assertProject } from '@pnpm/assert-project'
import { prepareEmpty } from '@pnpm/prepare'
import { filterProjectsBySelectorObjectsFromDir } from '@pnpm/workspace.projects-filter'
import { readYamlFileSync } from 'read-yaml-file'

import { DEFAULT_OPTS } from './utils/index.js'

const originalModule = await import('@pnpm/logger')
jest.unstable_mockModule('@pnpm/logger', () => ({
  ...originalModule,
  globalWarn: jest.fn(),
}))
const { globalWarn } = await import('@pnpm/logger')
const { importCommand } = await import('@pnpm/installing.commands')

const IS_POSITIVE_PATCH = path.join(import.meta.dirname, '../../deps-installer/test/fixtures/patch-pkg/is-positive@1.0.0.patch')

const PATCH_PATH = '.yarn/patches/is-positive-npm-1.0.0-0a1b2c3d4e.patch'

const YARN_LOCKFILE = `__metadata:
  version: 8
  cacheKey: 10c0

"is-positive@npm:1.0.0":
  version: 1.0.0
  resolution: "is-positive@npm:1.0.0"
  languageName: node
  linkType: hard
`

beforeEach(() => {
  jest.mocked(globalWarn).mockClear()
})

function prepareYarnProject (): void {
  prepareEmpty()
  fs.writeFileSync('package.json', JSON.stringify({
    name: 'root',
    dependencies: { 'is-positive': `patch:is-positive@npm%3A1.0.0#~/${PATCH_PATH}` },
  }))
  fs.writeFileSync('yarn.lock', YARN_LOCKFILE)
}

function readManifest (): { dependencies: Record<string, string> } {
  return JSON.parse(fs.readFileSync('package.json', 'utf8'))
}

test('import converts a yarn patch into patchedDependencies', async () => {
  prepareYarnProject()
  fs.mkdirSync(path.dirname(PATCH_PATH), { recursive: true })
  fs.copyFileSync(IS_POSITIVE_PATCH, PATCH_PATH)

  await importCommand.handler({ ...DEFAULT_OPTS, dir: process.cwd() }, [])

  expect(readManifest().dependencies['is-positive']).toBe('1.0.0')
  expect(readYamlFileSync<{ patchedDependencies: Record<string, string> }>('pnpm-workspace.yaml').patchedDependencies)
    .toStrictEqual({ 'is-positive@1.0.0': PATCH_PATH })
  const lockfile = assertProject(process.cwd()).readLockfile()
  expect(Object.keys(lockfile.patchedDependencies ?? {})).toStrictEqual(['is-positive@1.0.0'])
  expect(lockfile.importers['.'].dependencies?.['is-positive'].version).toMatch(/^1\.0\.0\(patch_hash=/)
  expect(globalWarn).not.toHaveBeenCalled()
})

test('import warns about a missing yarn patch file', async () => {
  prepareYarnProject()

  await importCommand.handler({ ...DEFAULT_OPTS, dir: process.cwd() }, [])

  expect(readManifest().dependencies['is-positive']).toBe('1.0.0')
  expect(fs.existsSync('pnpm-workspace.yaml')).toBe(false)
  const lockfile = assertProject(process.cwd()).readLockfile()
  expect(lockfile.importers['.'].dependencies?.['is-positive'].version).toBe('1.0.0')
  expect(globalWarn).toHaveBeenCalledWith(
    `The patch file ${path.resolve(PATCH_PATH)} of "is-positive" does not exist. "is-positive" was imported without the patch.`
  )
})

test('import warns about a dependency with several yarn patches', async () => {
  prepareEmpty()
  fs.writeFileSync('package.json', JSON.stringify({
    name: 'root',
    dependencies: { 'is-positive': 'patch:is-positive@npm%3A1.0.0#optional!builtin<compat/is-positive>&./a.patch&./b.patch' },
  }))
  fs.writeFileSync('yarn.lock', YARN_LOCKFILE)
  fs.copyFileSync(IS_POSITIVE_PATCH, 'a.patch')
  fs.copyFileSync(IS_POSITIVE_PATCH, 'b.patch')

  await importCommand.handler({ ...DEFAULT_OPTS, dir: process.cwd() }, [])

  expect(readManifest().dependencies['is-positive']).toBe('1.0.0')
  expect(fs.existsSync('pnpm-workspace.yaml')).toBe(false)
  expect(globalWarn).toHaveBeenCalledWith(
    '"is-positive" has several Yarn patches, and pnpm applies one patch per dependency. "is-positive" was imported without the patches.'
  )
})

test('import converts the yarn patches of workspace projects and warns about a conflicting patch', async () => {
  prepareEmpty()
  fs.writeFileSync('pnpm-workspace.yaml', 'packages:\n  - packages/*\n')
  fs.writeFileSync('package.json', JSON.stringify({
    name: 'root',
    dependencies: { 'is-positive': `patch:is-positive@npm%3A1.0.0#~/${PATCH_PATH}` },
  }))
  fs.mkdirSync(path.dirname(PATCH_PATH), { recursive: true })
  fs.copyFileSync(IS_POSITIVE_PATCH, PATCH_PATH)
  fs.mkdirSync('packages/foo', { recursive: true })
  fs.writeFileSync('packages/foo/package.json', JSON.stringify({
    name: 'foo',
    devDependencies: {
      positive: 'patch:positive@npm%3Ais-positive@1.0.0#./foo.patch::version=1.0.0&hash=abc',
    },
  }))
  fs.copyFileSync(IS_POSITIVE_PATCH, 'packages/foo/foo.patch')
  fs.writeFileSync('yarn.lock', YARN_LOCKFILE)
  const { allProjects, allProjectsGraph, selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])

  await importCommand.handler({
    ...DEFAULT_OPTS,
    allProjects: allProjects as any, // eslint-disable-line @typescript-eslint/no-explicit-any
    allProjectsGraph,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
    lockfileDir: process.cwd(),
    dir: process.cwd(),
  }, [])

  expect(readManifest().dependencies['is-positive']).toBe('1.0.0')
  expect(JSON.parse(fs.readFileSync('packages/foo/package.json', 'utf8')).devDependencies.positive).toBe('npm:is-positive@1.0.0')
  expect(readYamlFileSync<{ patchedDependencies: Record<string, string> }>('pnpm-workspace.yaml').patchedDependencies)
    .toStrictEqual({ 'is-positive@1.0.0': PATCH_PATH })
  const lockfile = assertProject(process.cwd()).readLockfile()
  expect(lockfile.importers['packages/foo'].devDependencies?.positive.version).toMatch(/^is-positive@1\.0\.0\(patch_hash=/)
  expect(globalWarn).toHaveBeenCalledWith(
    `The Yarn patch packages/foo/foo.patch of "positive" was not applied, because "is-positive@1.0.0" already uses the patch ${PATCH_PATH}.`
  )
})

test('import keeps a configured patch when the yarn patch file is missing', async () => {
  prepareYarnProject()
  fs.mkdirSync('patches')
  fs.copyFileSync(IS_POSITIVE_PATCH, 'patches/is-positive.patch')

  await importCommand.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    patchedDependencies: { 'is-positive@1.0.0': path.resolve('patches/is-positive.patch') },
  }, [])

  expect(readManifest().dependencies['is-positive']).toBe('1.0.0')
  const lockfile = assertProject(process.cwd()).readLockfile()
  expect(lockfile.importers['.'].dependencies?.['is-positive'].version).toMatch(/^1\.0\.0\(patch_hash=/)
  expect(globalWarn).not.toHaveBeenCalled()
})
