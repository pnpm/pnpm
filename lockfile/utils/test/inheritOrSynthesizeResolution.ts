import { expect, test } from '@jest/globals'
import type { PackageSnapshot, PackageSnapshots } from '@pnpm/lockfile.utils'
import { inheritOrSynthesizeResolution } from '@pnpm/lockfile.utils'
import type { DepPath } from '@pnpm/types'

const BASE_PATH = 'foo@file:packages/foo' as DepPath
const VARIANT_PATH = 'foo@file:packages/foo(peer@2.0.0)' as DepPath

test('returns the snapshot untouched when resolution is already populated', () => {
  const input = { resolution: { integrity: 'AAAA' } } as PackageSnapshot
  expect(inheritOrSynthesizeResolution('foo@1.0.0', input, undefined)).toBe(input)
})

test('inherits resolution from the base entry for peer-variant snapshots', () => {
  const baseResolution = { directory: 'packages/foo', type: 'directory' as const }
  const packages: PackageSnapshots = {
    [BASE_PATH]: { resolution: baseResolution } as PackageSnapshot,
    [VARIANT_PATH]: {} as PackageSnapshot,
  }
  const variant = packages[VARIANT_PATH]
  const out = inheritOrSynthesizeResolution(VARIANT_PATH, variant, packages)
  expect(out).not.toBe(variant)
  expect(out.resolution).toEqual(baseResolution)
})

test('synthesizes a directory resolution from a file: depPath when base is pruned', () => {
  // Mimics `turbo prune --docker` keeping only the variant entry.
  const variant = {} as PackageSnapshot
  const packages: PackageSnapshots = { [VARIANT_PATH]: variant }
  const out = inheritOrSynthesizeResolution(VARIANT_PATH, variant, packages)
  expect(out.resolution).toEqual({ directory: 'packages/foo', type: 'directory' })
})

test('returns the input untouched when resolution cannot be inherited or synthesized', () => {
  // Non-file:, non-peer-variant depPath with no base entry to inherit from.
  const input = {} as PackageSnapshot
  expect(inheritOrSynthesizeResolution('foo@1.0.0', input, undefined)).toBe(input)
})

test('does not synthesize a directory resolution for local-tarball file: refs', () => {
  // `file:foo.tgz`, `file:foo.tar.gz`, `file:foo.tar` are local-tarball refs
  // (refIsLocalTarball) — they must NOT be turned into `{ type: 'directory' }`.
  // Without a base to inherit from, the synthesis branch must skip them and
  // return the input untouched so downstream code can flag the broken lockfile
  // instead of silently routing through the wrong code path.
  for (const tarballPath of [
    'foo@file:foo.tgz' as DepPath,
    'foo@file:foo.tar.gz' as DepPath,
    'foo@file:foo.tar' as DepPath,
  ]) {
    const input = {} as PackageSnapshot
    const out = inheritOrSynthesizeResolution(tarballPath, input, undefined)
    expect(out).toBe(input)
    expect(out.resolution).toBeUndefined()
  }
})
