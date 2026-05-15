import { expect, test } from '@jest/globals'
import type { PackageSnapshot, PackageSnapshots } from '@pnpm/lockfile.utils'
import { inheritOrSynthesizeResolution } from '@pnpm/lockfile.utils'

test('returns the snapshot untouched when resolution is already populated', () => {
  const input = { resolution: { integrity: 'AAAA' } } as PackageSnapshot
  expect(inheritOrSynthesizeResolution('foo@1.0.0', input, undefined)).toBe(input)
})

test('inherits resolution from the base entry for peer-variant snapshots', () => {
  const baseResolution = { directory: 'packages/foo', type: 'directory' as const }
  const packages: PackageSnapshots = {
    'foo@file:packages/foo': { resolution: baseResolution } as PackageSnapshot,
    'foo@file:packages/foo(peer@2.0.0)': {} as PackageSnapshot,
  }
  const variant = packages['foo@file:packages/foo(peer@2.0.0)']
  const out = inheritOrSynthesizeResolution('foo@file:packages/foo(peer@2.0.0)', variant, packages)
  expect(out).not.toBe(variant)
  expect(out.resolution).toEqual(baseResolution)
})

test('synthesizes a directory resolution from a file: depPath when base is pruned', () => {
  // Mimics `turbo prune --docker` keeping only the variant entry.
  const variant = {} as PackageSnapshot
  const packages: PackageSnapshots = { 'foo@file:packages/foo(peer@2.0.0)': variant }
  const out = inheritOrSynthesizeResolution('foo@file:packages/foo(peer@2.0.0)', variant, packages)
  expect(out.resolution).toEqual({ directory: 'packages/foo', type: 'directory' })
})

test('returns the input untouched when resolution cannot be inherited or synthesized', () => {
  // Non-file:, non-peer-variant depPath with no base entry to inherit from.
  const input = {} as PackageSnapshot
  expect(inheritOrSynthesizeResolution('foo@1.0.0', input, undefined)).toBe(input)
})
