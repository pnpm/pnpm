/// <reference path="../../../__typings__/index.d.ts"/>
import { promises as fs } from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { arrayOfWorkspacePackagesToMap, getContext } from '@pnpm/installing.context'
import type { ProjectId, ProjectRootDir } from '@pnpm/types'

import type { GetContextOptions } from '../src/index.js'
import { readLockfiles } from '../src/readLockfiles.js'

const DEFAULT_OPTIONS: GetContextOptions = {
  allProjects: [],
  autoInstallPeers: true,
  excludeLinksFromLockfile: false,
  extraBinPaths: [],
  force: false,
  lockfileDir: path.join(import.meta.dirname, 'lockfile'),
  nodeLinker: 'isolated',
  hoistPattern: ['*'],
  registries: { default: '' },
  useLockfile: false,
  include: {
    dependencies: true,
    devDependencies: true,
    optionalDependencies: true,
  },
  storeDir: path.join(import.meta.dirname, 'store'),
  virtualStoreDirMaxLength: 120,
  peersSuffixMaxLength: 1000,
}

test('getContext - extendNodePath false', async () => {
  const context = await getContext({
    ...DEFAULT_OPTIONS,
    extendNodePath: false,
  })
  expect(context.extraNodePaths).toEqual([])
})

test('getContext - extendNodePath true', async () => {
  const context = await getContext({
    ...DEFAULT_OPTIONS,
    extendNodePath: true,
  })
  expect(context.extraNodePaths).toEqual([path.join(context.virtualStoreDir, 'node_modules')])
})

// This is supported for compatibility with Yarn's implementation
// see https://github.com/pnpm/pnpm/issues/2648
test('arrayOfWorkspacePackagesToMap() treats private packages with no version as packages with 0.0.0 version', () => {
  const privateProject = {
    rootDir: process.cwd() as ProjectRootDir,
    manifest: {
      name: 'private-pkg',
    },
  }
  expect(arrayOfWorkspacePackagesToMap([privateProject])).toStrictEqual(new Map([
    ['private-pkg', new Map([
      ['0.0.0', privateProject],
    ])],
  ]))
})

test('readLockfiles() throws on incompatible lockfile in CI when frozenLockfile is true', async () => {
  const lockfileDir = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-get-context-'))
  await fs.writeFile(path.join(lockfileDir, 'pnpm-lock.yaml'), 'lockfileVersion: 1.0\nimporters:\n  .:\n    specifiers: {}\n')

  await expect(readLockfiles({
    autoInstallPeers: true,
    excludeLinksFromLockfile: false,
    peersSuffixMaxLength: 1000,
    ci: true,
    force: false,
    frozenLockfile: true,
    projects: [{ id: '.' as ProjectId, rootDir: lockfileDir as ProjectRootDir }],
    lockfileDir,
    registry: 'https://registry.npmjs.org/',
    useLockfile: true,
    internalPnpmDir: path.join(lockfileDir, 'node_modules', '.pnpm'),
  })).rejects.toMatchObject({ code: 'ERR_PNPM_LOCKFILE_BREAKING_CHANGE' })
})

test('readLockfiles() ignores incompatible lockfile in CI when frozenLockfile is false', async () => {
  const lockfileDir = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-get-context-'))
  await fs.writeFile(path.join(lockfileDir, 'pnpm-lock.yaml'), 'lockfileVersion: 1.0\nimporters:\n  .:\n    specifiers: {}\n')

  const context = await readLockfiles({
    autoInstallPeers: true,
    excludeLinksFromLockfile: false,
    peersSuffixMaxLength: 1000,
    ci: true,
    force: false,
    frozenLockfile: false,
    projects: [{ id: '.' as ProjectId, rootDir: lockfileDir as ProjectRootDir }],
    lockfileDir,
    registry: 'https://registry.npmjs.org/',
    useLockfile: true,
    internalPnpmDir: path.join(lockfileDir, 'node_modules', '.pnpm'),
  })

  expect(context.existsWantedLockfile).toBe(false)
})
