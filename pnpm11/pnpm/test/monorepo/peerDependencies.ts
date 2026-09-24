import path from 'node:path'

import { expect, test } from '@jest/globals'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import type { LockfileFile } from '@pnpm/lockfile.types'
import { readPackageJsonFromDir } from '@pnpm/pkg-manifest.reader'
import { preparePackages } from '@pnpm/prepare'
import { readYamlFileSync } from 'read-yaml-file'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm } from '../utils/index.js'

// Covers https://github.com/pnpm/pnpm/issues/6272
test('peer dependency is not unlinked when adding a new dependency', async () => {
  preparePackages([
    {
      name: 'project-1',

      dependencies: {
        '@pnpm.e2e/abc': '1.0.0',
        '@pnpm.e2e/peer-a': 'workspace:*',
      },
    },
    {
      name: '@pnpm.e2e/peer-a',
      version: '1.0.0',

      dependencies: {},
    },
  ])

  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['**', '!store/**'],
    autoInstallPeers: false,
  })
  await execPnpm(['install'])
  await execPnpm(['--filter=project-1', 'add', 'is-odd@1.0.0'])

  const lockfile = readYamlFileSync<LockfileFile>(WANTED_LOCKFILE)
  expect(Object.keys(lockfile!.snapshots!)).toContain('@pnpm.e2e/abc@1.0.0(@pnpm.e2e/peer-a@@pnpm.e2e+peer-a)')
})

test('pnpm add --save-peer in a workspace package saves to peerDependencies and devDependencies', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
    },
  ])

  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['**', '!store/**'],
  })
  await execPnpm(['--filter=project-1', 'add', '--save-peer', 'is-positive@1.0.0'])

  const manifest = await readPackageJsonFromDir(path.resolve('project-1'))
  expect(manifest.peerDependencies).toEqual({ 'is-positive': '1.0.0' })
  expect(manifest.devDependencies).toEqual({ 'is-positive': '1.0.0' })
})

test('pnpm add -P in a workspace package saves to peerDependencies and devDependencies', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
    },
  ])

  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['**', '!store/**'],
  })
  await execPnpm(['--filter=project-1', 'add', '-P', 'is-positive@1.0.0'])

  const manifest = await readPackageJsonFromDir(path.resolve('project-1'))
  expect(manifest.peerDependencies).toEqual({ 'is-positive': '1.0.0' })
  expect(manifest.devDependencies).toEqual({ 'is-positive': '1.0.0' })
})

test('pnpm add --save-peer from inside workspace package directory saves to peerDependencies and devDependencies', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
    },
  ])

  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['**', '!store/**'],
  })
  process.chdir('project-1')
  await execPnpm(['add', '--save-peer', 'is-positive@1.0.0'])

  const manifest = await readPackageJsonFromDir(process.cwd())
  expect(manifest.peerDependencies).toEqual({ 'is-positive': '1.0.0' })
  expect(manifest.devDependencies).toEqual({ 'is-positive': '1.0.0' })
})

test('pnpm add -P from inside workspace package directory saves to peerDependencies and devDependencies', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
    },
  ])

  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['**', '!store/**'],
  })
  process.chdir('project-1')
  await execPnpm(['add', '-P', 'is-positive@1.0.0'])

  const manifest = await readPackageJsonFromDir(process.cwd())
  expect(manifest.peerDependencies).toEqual({ 'is-positive': '1.0.0' })
  expect(manifest.devDependencies).toEqual({ 'is-positive': '1.0.0' })
})
