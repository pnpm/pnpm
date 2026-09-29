import { expect, test } from '@jest/globals'
import type { ProjectManifest } from '@pnpm/types'

import { inheritPackageManager } from '../../src/deploy/inheritPackageManager.js'

function inheritedPackageManagerOf (root: ProjectManifest): string | undefined {
  return inheritPackageManager({ name: 'app' }, root).packageManager
}

test('inheritPackageManager keeps the integrity hash of a matching root pin', () => {
  expect(inheritedPackageManagerOf({
    packageManager: 'pnpm@10.18.0+sha512.abc123',
    devEngines: { packageManager: { name: 'pnpm', version: '10.18.0' } },
  })).toBe('pnpm@10.18.0+sha512.abc123')
})

test('inheritPackageManager keeps the integrity hash of an exact devEngines pin', () => {
  expect(inheritedPackageManagerOf({
    packageManager: 'pnpm@10.18.0+sha512.def456',
    devEngines: { packageManager: { name: 'pnpm', version: '10.18.0+sha512.abc123' } },
  })).toBe('pnpm@10.18.0+sha512.abc123')
})

test('inheritPackageManager prefers an exact devEngines pin over another version', () => {
  expect(inheritedPackageManagerOf({
    packageManager: 'pnpm@10.17.0+sha512.abc123',
    devEngines: { packageManager: { name: 'pnpm', version: '10.18.0' } },
  })).toBe('pnpm@10.18.0')
})

test('inheritPackageManager ignores a devEngines pin of another package manager', () => {
  expect(inheritedPackageManagerOf({
    packageManager: 'pnpm@10.18.0',
    devEngines: { packageManager: { name: 'npm', version: '10.9.0' } },
  })).toBe('pnpm@10.18.0')
  expect(inheritedPackageManagerOf({
    devEngines: { packageManager: { name: 'npm', version: '10.9.0' } },
  })).toBeUndefined()
})

test('inheritPackageManager reads the pnpm entry of a devEngines list', () => {
  expect(inheritedPackageManagerOf({
    devEngines: {
      packageManager: [
        { name: 'npm', version: '10.9.0' },
        { name: 'pnpm', version: '10.18.0' },
      ],
    },
  })).toBe('pnpm@10.18.0')
})
