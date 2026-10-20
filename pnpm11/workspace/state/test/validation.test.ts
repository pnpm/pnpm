import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { prepareEmpty } from '@pnpm/prepare'
import type { ProjectRootDir } from '@pnpm/types'

import { fileDigestOf, loadWorkspaceState, manifestDigest, type ProjectsList, projectValidation, sharedInputPaths, updateWorkspaceState, type Validation } from '../src/index.js'

test('manifestDigest() ignores key order', () => {
  expect(manifestDigest({ name: 'a', dependencies: { x: '1', y: '2' } }))
    .toBe(manifestDigest({ dependencies: { y: '2', x: '1' }, name: 'a' }))
  expect(manifestDigest({ name: 'a', dependencies: { x: '1', y: '2' } }))
    .not.toBe(manifestDigest({ name: 'a', dependencies: { x: '1', y: '3' } }))
})

// Strings are length-prefixed, so moving a character from a key into its
// value changes the digest.
test('manifestDigest() separates keys from values', () => {
  expect(manifestDigest({ a: 'bc' })).not.toBe(manifestDigest({ ab: 'c' }))
})

// pnpm v12 pins the same value, so either CLI recognizes what the other
// recorded.
test('manifestDigest() matches the digest pnpm v12 computes', () => {
  expect(manifestDigest({
    name: 'a',
    version: '1.0.0',
    dependencies: { '@scope/x': '^1.0.0', y: '2' },
    peerDependenciesMeta: { y: { optional: true } },
    files: ['lib'],
    weird: [1, 2.5, null, false, 'é'],
  })).toBe('0684f5a6122e59859b6c170262a4c110')
})

// A file written in the clock tick its stat was recorded in may have been
// written again without its stat moving, so its content is read again.
test('fileDigestOf() reads a file whose stat was recorded in the same tick again', () => {
  prepareEmpty()
  const filePath = path.resolve('pnpm-lock.yaml')
  fs.writeFileSync(filePath, "lockfileVersion: '9.0'\n")
  const actual = fileDigestOf(undefined, filePath)
  if (actual.kind !== 'present') throw new Error('expected a digest')
  const recorded = (recordedAtNs: number): Validation => ({
    recordedAtNs,
    files: { [filePath]: { ...actual.signature, digest: 'recorded' } },
    manifests: {},
  })
  const changedNs = Math.max(actual.signature.mtimeNs, actual.signature.ctimeNs ?? 0)
  expect(fileDigestOf(recorded(changedNs + 5_000_000_000), filePath)).toStrictEqual({
    kind: 'present',
    signature: { ...actual.signature, digest: 'recorded' },
  })
  expect(fileDigestOf(recorded(changedNs), filePath)).toStrictEqual(actual)
})

// A filtered install vouches for the projects it installed and carries the
// records of the others, unless an input every project shares changed.
test('updateWorkspaceState() carries the records of the projects a filtered install left out', async () => {
  prepareEmpty()
  const workspaceDir = process.cwd()
  fs.writeFileSync('pnpm-lock.yaml', "lockfileVersion: '9.0'\n")
  const allProjects = ['a', 'b'].map((name) => ({ rootDir: path.join(workspaceDir, name) as ProjectRootDir, manifest: { name } }))
  const write = async (dedupePeers: boolean, vouchedProjects?: string[]) => {
    await updateWorkspaceState({
      allProjects,
      settings: { excludeLinksFromLockfile: false, linkWorkspacePackages: false, preferWorkspacePackages: false, dedupePeers },
      workspaceDir,
      pnpmfiles: [],
      filteredInstall: vouchedProjects != null,
      validation: { lockfileDir: workspaceDir, wantedLockfileName: 'pnpm-lock.yaml', sharedInputs: [], vouchedProjects },
    })
    return loadWorkspaceState(workspaceDir)!
  }
  const installed = await write(false)
  expect(Object.keys(installed.validation!.manifests)).toStrictEqual(['a', 'b'])

  const filtered = await write(false, [allProjects[0].rootDir])
  expect(Object.keys(filtered.validation!.manifests)).toStrictEqual(['a', 'b'])
  expect(projectValidation(filtered.validation!, 'b')?.manifest).toBe(manifestDigest({ name: 'b' }))
  expect(filtered.filteredInstall).toBe(false)

  const drifted = await write(true, [allProjects[0].rootDir])
  expect(Object.keys(drifted.validation!.manifests)).toStrictEqual(['a'])
  expect(drifted.filteredInstall).toBe(true)
})

test('sharedInputPaths() lists no pnpmfile when --ignore-pnpmfile left them unloaded', () => {
  expect(sharedInputPaths('/repo', { foo: 'patches/foo.patch' }, undefined)).toStrictEqual([path.resolve('/repo', 'patches/foo.patch')])
})

test('updateWorkspaceState() warns instead of failing when the previous state cannot be read', async () => {
  prepareEmpty()
  const workspaceDir = process.cwd()
  fs.writeFileSync('pnpm-lock.yaml', "lockfileVersion: '9.0'\n")
  fs.mkdirSync('node_modules/.pnpm-workspace-state-v1.json', { recursive: true })
  const rootDir = path.join(workspaceDir, 'a') as ProjectRootDir
  await expect(updateWorkspaceState({
    allProjects: [{ rootDir, manifest: { name: 'a' } }],
    settings: { excludeLinksFromLockfile: false, linkWorkspacePackages: false, preferWorkspacePackages: false },
    workspaceDir,
    pnpmfiles: [],
    filteredInstall: true,
    validation: { lockfileDir: workspaceDir, wantedLockfileName: 'pnpm-lock.yaml', sharedInputs: [], vouchedProjects: [rootDir] },
  })).resolves.toBeUndefined()
})

test('updateWorkspaceState() preserves autoDedupe and ignoredWorkspaceStateSettings across refreshes and filtered installs', async () => {
  prepareEmpty()
  const workspaceDir = process.cwd()
  fs.writeFileSync('pnpm-lock.yaml', "lockfileVersion: '9.0'\n")
  const rootDir = path.join(workspaceDir, 'a') as ProjectRootDir
  const allProjects = [{ rootDir, manifest: { name: 'a' } }]
  await updateWorkspaceState({
    allProjects,
    settings: {
      excludeLinksFromLockfile: false,
      linkWorkspacePackages: false,
      preferWorkspacePackages: false,
      autoDedupe: true,
      dev: false,
      optional: true,
      production: true,
    },
    workspaceDir,
    pnpmfiles: [],
    filteredInstall: false,
    validation: { lockfileDir: workspaceDir, wantedLockfileName: 'pnpm-lock.yaml', sharedInputs: [] },
  })

  await updateWorkspaceState({
    allProjects,
    settings: {
      excludeLinksFromLockfile: false,
      linkWorkspacePackages: false,
      preferWorkspacePackages: false,
      dev: true,
      optional: false,
      production: false,
    },
    workspaceDir,
    pnpmfiles: [],
    filteredInstall: false,
    ignoredWorkspaceStateSettings: ['dev', 'optional', 'production'],
    validation: { lockfileDir: workspaceDir, wantedLockfileName: 'pnpm-lock.yaml', sharedInputs: [], vouchedProjects: [rootDir] },
  })

  const state = loadWorkspaceState(workspaceDir)!
  expect(state.settings.autoDedupe).toBe(true)
  expect(state.settings.dev).toBe(false)
  expect(state.settings.optional).toBe(true)
  expect(state.settings.production).toBe(true)

  await updateWorkspaceState({
    allProjects,
    settings: {
      excludeLinksFromLockfile: false,
      linkWorkspacePackages: false,
      preferWorkspacePackages: false,
    },
    workspaceDir,
    pnpmfiles: [],
    filteredInstall: false,
    validation: { lockfileDir: workspaceDir, wantedLockfileName: 'pnpm-lock.yaml', sharedInputs: [] },
  })
  expect(loadWorkspaceState(workspaceDir)!.settings.autoDedupe).toBe(true)
})

test('updateWorkspaceState() records per-project lockfiles when sharedWorkspaceLockfile is false', async () => {
  prepareEmpty()
  const workspaceDir = process.cwd()
  const dirA = path.join(workspaceDir, 'packages/a') as ProjectRootDir
  const dirB = path.join(workspaceDir, 'packages/b') as ProjectRootDir
  fs.mkdirSync(dirA, { recursive: true })
  fs.mkdirSync(dirB, { recursive: true })
  fs.writeFileSync(path.join(dirA, 'pnpm-lock.yaml'), "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      foo:\n        specifier: 1.0.0\n        version: 1.0.0\n")
  fs.writeFileSync(path.join(dirB, 'pnpm-lock.yaml'), "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      bar:\n        specifier: 1.0.0\n        version: 1.0.0\n")
  const allProjects: ProjectsList = [
    { rootDir: dirA, manifest: { name: 'a', dependencies: { foo: '1.0.0' } } },
    { rootDir: dirB, manifest: { name: 'b', dependencies: { bar: '1.0.0' } } },
  ]
  const write = async (vouchedProjects?: string[]) => {
    await updateWorkspaceState({
      allProjects,
      settings: { excludeLinksFromLockfile: false, linkWorkspacePackages: false, preferWorkspacePackages: false },
      workspaceDir,
      pnpmfiles: [],
      filteredInstall: vouchedProjects != null,
      validation: {
        lockfileDir: workspaceDir,
        wantedLockfileName: 'pnpm-lock.yaml',
        sharedInputs: [],
        vouchedProjects,
        sharedWorkspaceLockfile: false,
      },
    })
    return loadWorkspaceState(workspaceDir)!
  }
  const full = await write()
  expect(full.filteredInstall).toBe(false)
  expect(Object.keys(full.validation!.manifests)).toStrictEqual(['packages/a', 'packages/b'])
  expect(full.validation!.lockfiles?.['packages/a']).toBeDefined()
  expect(full.validation!.lockfiles?.['packages/b']).toBeDefined()
  expect(full.validation!.lockfiles?.['packages/a']).not.toBe(full.validation!.lockfiles?.['packages/b'])

  const filtered = await write([dirA])
  expect(filtered.filteredInstall).toBe(false)
  expect(projectValidation(filtered.validation!, 'packages/b')?.lockfile).toBe(full.validation!.lockfiles?.['packages/b'])
})
