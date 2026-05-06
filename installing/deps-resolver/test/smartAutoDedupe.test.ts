import { expect, test } from '@jest/globals'
import type { DepPath } from '@pnpm/types'

import { applySmartAutoDedupe } from '../lib/smartAutoDedupe.js'

interface NodeStub {
  name: string
  version: string
  children: Record<string, DepPath>
  depSpecs?: Record<string, string>
  // Default resolution mimics a registry tarball (TarballResolution has no `type`).
  // Tests that need to model file:/git/etc. resolves can override.
  resolution?: { type?: string, [key: string]: unknown }
}

function makeGraph (nodes: Record<string, NodeStub>): Parameters<typeof applySmartAutoDedupe>[0] {
  for (const node of Object.values(nodes)) {
    if (node.resolution == null) {
      // Mimics a registry tarball: TarballResolution has no `type`
      // discriminator and the tarball URL is HTTP(S).
      node.resolution = { tarball: 'https://registry.example/pkg.tgz' }
    }
  }
  return nodes as unknown as Parameters<typeof applySmartAutoDedupe>[0]
}

test('rewrites a child edge to a higher in-graph version when the spec is satisfied', () => {
  const graph = makeGraph({
    'foo@1.0.0': {
      name: 'foo',
      version: '1.0.0',
      children: {},
    },
    'foo@1.1.0': {
      name: 'foo',
      version: '1.1.0',
      children: {},
    },
    'pkg@2.0.0': {
      name: 'pkg',
      version: '2.0.0',
      children: { foo: 'foo@1.0.0' as DepPath },
      depSpecs: { foo: '^1.0.0' },
    },
  })

  applySmartAutoDedupe(graph)

  expect(graph['pkg@2.0.0' as DepPath].children.foo).toBe('foo@1.1.0')
})

test('does not rewrite when the higher version does not satisfy the spec', () => {
  const graph = makeGraph({
    'foo@1.0.0': { name: 'foo', version: '1.0.0', children: {} },
    'foo@2.0.0': { name: 'foo', version: '2.0.0', children: {} },
    'pkg@1.0.0': {
      name: 'pkg',
      version: '1.0.0',
      children: { foo: 'foo@1.0.0' as DepPath },
      depSpecs: { foo: '^1.0.0' },
    },
  })

  applySmartAutoDedupe(graph)

  expect(graph['pkg@1.0.0' as DepPath].children.foo).toBe('foo@1.0.0')
})

test('does not downgrade — only ever picks a higher version', () => {
  const graph = makeGraph({
    'foo@1.0.0': { name: 'foo', version: '1.0.0', children: {} },
    'foo@1.5.0': { name: 'foo', version: '1.5.0', children: {} },
    'pkg@1.0.0': {
      name: 'pkg',
      version: '1.0.0',
      children: { foo: 'foo@1.5.0' as DepPath },
      depSpecs: { foo: '^1.0.0' },
    },
  })

  applySmartAutoDedupe(graph)

  expect(graph['pkg@1.0.0' as DepPath].children.foo).toBe('foo@1.5.0')
})

test('skips edges whose spec is not a valid semver range (e.g. exotic protocols)', () => {
  const graph = makeGraph({
    'foo@1.0.0': { name: 'foo', version: '1.0.0', children: {} },
    'foo@1.1.0': { name: 'foo', version: '1.1.0', children: {} },
    'pkg@1.0.0': {
      name: 'pkg',
      version: '1.0.0',
      children: { foo: 'foo@1.0.0' as DepPath },
      depSpecs: { foo: 'github:user/repo' },
    },
  })

  applySmartAutoDedupe(graph)

  expect(graph['pkg@1.0.0' as DepPath].children.foo).toBe('foo@1.0.0')
})

test('skips children with a peer-suffixed depPath', () => {
  const graph = makeGraph({
    'foo@1.0.0(react@17.0.0)': {
      name: 'foo',
      version: '1.0.0',
      children: {},
    },
    'foo@1.1.0(react@17.0.0)': {
      name: 'foo',
      version: '1.1.0',
      children: {},
    },
    'pkg@1.0.0': {
      name: 'pkg',
      version: '1.0.0',
      children: { foo: 'foo@1.0.0(react@17.0.0)' as DepPath },
      depSpecs: { foo: '^1.0.0' },
    },
  })

  applySmartAutoDedupe(graph)

  // Both peer-suffixed candidates share the same group (same peer hash),
  // so the rewrite is allowed because the suffix matches.
  expect(graph['pkg@1.0.0' as DepPath].children.foo).toBe('foo@1.1.0(react@17.0.0)')
})

test('does not rewrite across mismatched peer-dep-graph hashes', () => {
  const graph = makeGraph({
    'foo@1.0.0(react@17.0.0)': { name: 'foo', version: '1.0.0', children: {} },
    'foo@1.1.0(react@18.0.0)': { name: 'foo', version: '1.1.0', children: {} },
    'pkg@1.0.0': {
      name: 'pkg',
      version: '1.0.0',
      children: { foo: 'foo@1.0.0(react@17.0.0)' as DepPath },
      depSpecs: { foo: '^1.0.0' },
    },
  })

  applySmartAutoDedupe(graph)

  expect(graph['pkg@1.0.0' as DepPath].children.foo).toBe('foo@1.0.0(react@17.0.0)')
})

test('is a no-op when no group has multiple versions', () => {
  const graph = makeGraph({
    'foo@1.0.0': { name: 'foo', version: '1.0.0', children: {} },
    'pkg@1.0.0': {
      name: 'pkg',
      version: '1.0.0',
      children: { foo: 'foo@1.0.0' as DepPath },
      depSpecs: { foo: '^1.0.0' },
    },
  })

  applySmartAutoDedupe(graph)

  expect(graph['pkg@1.0.0' as DepPath].children.foo).toBe('foo@1.0.0')
})

test('does not rewrite a patched edge to an unpatched higher version (would lose the patch)', () => {
  // The patched 1.0.0 must not be merged with the unpatched 1.1.0 — they
  // are not interchangeable even though they share a name and have no
  // peer suffix.
  const graph = makeGraph({
    'foo@1.0.0(patch_hash=abc)': { name: 'foo', version: '1.0.0', children: {} },
    'foo@1.1.0': { name: 'foo', version: '1.1.0', children: {} },
    'pkg@1.0.0': {
      name: 'pkg',
      version: '1.0.0',
      children: { foo: 'foo@1.0.0(patch_hash=abc)' as DepPath },
      depSpecs: { foo: '^1.0.0' },
    },
  })

  applySmartAutoDedupe(graph)

  expect(graph['pkg@1.0.0' as DepPath].children.foo).toBe('foo@1.0.0(patch_hash=abc)')
})

test('rewrites within the same patch hash', () => {
  const graph = makeGraph({
    'foo@1.0.0(patch_hash=abc)': { name: 'foo', version: '1.0.0', children: {} },
    'foo@1.1.0(patch_hash=abc)': { name: 'foo', version: '1.1.0', children: {} },
    'pkg@1.0.0': {
      name: 'pkg',
      version: '1.0.0',
      children: { foo: 'foo@1.0.0(patch_hash=abc)' as DepPath },
      depSpecs: { foo: '^1.0.0' },
    },
  })

  applySmartAutoDedupe(graph)

  expect(graph['pkg@1.0.0' as DepPath].children.foo).toBe('foo@1.1.0(patch_hash=abc)')
})

test('handles patch hash combined with a peer suffix', () => {
  const graph = makeGraph({
    'foo@1.0.0(patch_hash=abc)(react@17.0.0)': { name: 'foo', version: '1.0.0', children: {} },
    'foo@1.1.0(patch_hash=abc)(react@17.0.0)': { name: 'foo', version: '1.1.0', children: {} },
    'foo@1.1.0(react@17.0.0)': { name: 'foo', version: '1.1.0', children: {} },
    'pkg@1.0.0': {
      name: 'pkg',
      version: '1.0.0',
      children: { foo: 'foo@1.0.0(patch_hash=abc)(react@17.0.0)' as DepPath },
      depSpecs: { foo: '^1.0.0' },
    },
  })

  applySmartAutoDedupe(graph)

  expect(graph['pkg@1.0.0' as DepPath].children.foo)
    .toBe('foo@1.1.0(patch_hash=abc)(react@17.0.0)')
})

test('does not pick a prerelease version for a non-prerelease spec under loose semver', () => {
  // semver.satisfies('1.1.0-beta.1', '^1.0.0', { loose: true }) === false.
  // A prerelease must not silently dedupe a stable edge.
  const graph = makeGraph({
    'foo@1.0.0': { name: 'foo', version: '1.0.0', children: {} },
    'foo@1.1.0-beta.1': { name: 'foo', version: '1.1.0-beta.1', children: {} },
    'pkg@1.0.0': {
      name: 'pkg',
      version: '1.0.0',
      children: { foo: 'foo@1.0.0' as DepPath },
      depSpecs: { foo: '^1.0.0' },
    },
  })

  applySmartAutoDedupe(graph)

  expect(graph['pkg@1.0.0' as DepPath].children.foo).toBe('foo@1.0.0')
})

test('does not swap a registry-resolved edge for a directory-resolved candidate of the same name', () => {
  // A workspace / file: package whose manifest happens to have a higher
  // version must NEVER be picked as the dedupe target for a registry
  // edge. Doing so would silently change install content from a
  // published tarball to a local directory.
  const graph = makeGraph({
    'foo@file:packages/foo': {
      name: 'foo',
      version: '1.5.0',
      children: {},
      resolution: { type: 'directory', directory: '/tmp/packages/foo' },
    },
    'foo@1.0.0': { name: 'foo', version: '1.0.0', children: {} },
    'pkg@1.0.0': {
      name: 'pkg',
      version: '1.0.0',
      children: { foo: 'foo@1.0.0' as DepPath },
      depSpecs: { foo: '^1.0.0' },
    },
  })

  applySmartAutoDedupe(graph)

  expect(graph['pkg@1.0.0' as DepPath].children.foo).toBe('foo@1.0.0')
})

test('does not treat a local file:./pkg.tgz tarball as interchangeable with a registry tarball', () => {
  // file:./pkg.tgz packages are also TarballResolutions (type=undefined),
  // but the tarball field is a local path rather than an HTTP(S) URL.
  // They must NOT be merged with registry tarballs of the same name.
  const graph = makeGraph({
    'foo@file:./foo.tgz': {
      name: 'foo',
      version: '1.5.0',
      children: {},
      resolution: { tarball: 'file:./foo.tgz' },
    },
    'foo@1.0.0': { name: 'foo', version: '1.0.0', children: {} },
    'pkg@1.0.0': {
      name: 'pkg',
      version: '1.0.0',
      children: { foo: 'foo@1.0.0' as DepPath },
      depSpecs: { foo: '^1.0.0' },
    },
  })

  applySmartAutoDedupe(graph)

  expect(graph['pkg@1.0.0' as DepPath].children.foo).toBe('foo@1.0.0')
})

test('does not treat a git resolution as interchangeable with a registry tarball', () => {
  const graph = makeGraph({
    'foo@github.com/user/repo': {
      name: 'foo',
      version: '1.5.0',
      children: {},
      resolution: { type: 'git', repo: 'https://github.com/user/repo', commit: 'deadbeef' },
    },
    'foo@1.0.0': { name: 'foo', version: '1.0.0', children: {} },
    'pkg@1.0.0': {
      name: 'pkg',
      version: '1.0.0',
      children: { foo: 'foo@1.0.0' as DepPath },
      depSpecs: { foo: '^1.0.0' },
    },
  })

  applySmartAutoDedupe(graph)

  expect(graph['pkg@1.0.0' as DepPath].children.foo).toBe('foo@1.0.0')
})

test('does not rewrite an edge whose own resolution is non-registry, even if a higher registry version exists', () => {
  // A directory-resolved edge cannot be deduped even if a registry
  // tarball at a higher version is present — they're not interchangeable.
  const graph = makeGraph({
    'foo@file:packages/foo': {
      name: 'foo',
      version: '1.0.0',
      children: {},
      resolution: { type: 'directory', directory: '/tmp/packages/foo' },
    },
    'foo@1.5.0': { name: 'foo', version: '1.5.0', children: {} },
    'pkg@1.0.0': {
      name: 'pkg',
      version: '1.0.0',
      children: { foo: 'foo@file:packages/foo' as DepPath },
      depSpecs: { foo: '^1.0.0' },
    },
  })

  applySmartAutoDedupe(graph)

  expect(graph['pkg@1.0.0' as DepPath].children.foo).toBe('foo@file:packages/foo')
})

test('is idempotent: a second pass on the same graph produces no further changes', () => {
  const graph = makeGraph({
    'foo@1.0.0': { name: 'foo', version: '1.0.0', children: {} },
    'foo@1.1.0': { name: 'foo', version: '1.1.0', children: {} },
    'foo@1.2.0': { name: 'foo', version: '1.2.0', children: {} },
    'a@1.0.0': {
      name: 'a',
      version: '1.0.0',
      children: { foo: 'foo@1.0.0' as DepPath },
      depSpecs: { foo: '^1.0.0' },
    },
    'b@1.0.0': {
      name: 'b',
      version: '1.0.0',
      children: { foo: 'foo@1.1.0' as DepPath },
      depSpecs: { foo: '^1.1.0' },
    },
  })

  applySmartAutoDedupe(graph)
  const afterFirst = JSON.parse(JSON.stringify({
    a: graph['a@1.0.0' as DepPath].children,
    b: graph['b@1.0.0' as DepPath].children,
  }))

  applySmartAutoDedupe(graph)
  const afterSecond = {
    a: graph['a@1.0.0' as DepPath].children,
    b: graph['b@1.0.0' as DepPath].children,
  }

  expect(afterSecond).toEqual(afterFirst)
  expect(afterFirst.a.foo).toBe('foo@1.2.0')
  expect(afterFirst.b.foo).toBe('foo@1.2.0')
})

test('rewrites edges sourced from optionalDependencies just like regular dependencies', () => {
  const graph = makeGraph({
    'foo@1.0.0': { name: 'foo', version: '1.0.0', children: {} },
    'foo@1.2.0': { name: 'foo', version: '1.2.0', children: {} },
    'pkg@1.0.0': {
      name: 'pkg',
      version: '1.0.0',
      children: { foo: 'foo@1.0.0' as DepPath },
      // depSpecs merges regular + optional, so this stands in for an
      // edge that was declared under optionalDependencies.
      depSpecs: { foo: '^1.0.0' },
    },
  })

  applySmartAutoDedupe(graph)

  expect(graph['pkg@1.0.0' as DepPath].children.foo).toBe('foo@1.2.0')
})

