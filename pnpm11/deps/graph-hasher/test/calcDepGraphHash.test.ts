import { describe, expect, test } from '@jest/globals'
import { calcDepGraphHash, createDepGraphHashContext, type DepsGraph } from '@pnpm/deps.graph-hasher'

function node (id: string, children: Record<string, string> = {}) {
  return { fullPkgId: `${id}@1.0.0:sha512-${id}`, children }
}

function graphHash (depsGraph: DepsGraph<string>, depPath: string, cache = {}): string {
  return calcDepGraphHash({ depsGraph, cache, depPath, context: createDepGraphHashContext() })
}

describe('calcDepGraphHash() for dependency cycles', () => {
  // https://github.com/pnpm/pnpm/issues/17466
  test('every cycle member covers what the cycle reaches', () => {
    const withLeaf = (leafId: string) => ({
      core: node('core', { ws: 'ws', leaf: 'leaf' }),
      ws: node('ws', { core: 'core' }),
      leaf: node(leafId),
    })
    for (const depPath of ['core', 'ws']) {
      expect(graphHash(withLeaf('leaf-1'), depPath)).not.toBe(graphHash(withLeaf('leaf-2'), depPath))
    }
    const wsFirst: Record<string, string> = {}
    graphHash(withLeaf('leaf-1'), 'ws', wsFirst)
    expect(wsFirst.core).toBe(graphHash(withLeaf('leaf-1'), 'core'))
  })

  test('does not depend on query order', () => {
    const depsGraph = {
      a: node('a', { m: 'm', z: 'z', b: 'b' }),
      b: node('b', { a: 'a' }),
      m: node('m', { a: 'a' }),
      z: node('z'),
    }
    const forward: Record<string, string> = {}
    const backward: Record<string, string> = {}
    for (const depPath of ['a', 'b', 'm', 'z']) graphHash(depsGraph, depPath, forward)
    for (const depPath of ['z', 'm', 'b', 'a']) graphHash(depsGraph, depPath, backward)
    expect(forward).toStrictEqual(backward)
  })

  test('a self dependency covers its other dependencies', () => {
    const withLeaf = (leafId: string) => ({
      self: node('self', { self: 'self', leaf: 'leaf' }),
      leaf: node(leafId),
    })
    expect(graphHash(withLeaf('leaf-1'), 'self')).not.toBe(graphHash(withLeaf('leaf-2'), 'self'))
  })

  // a1 and a2 are one package in two peer contexts inside the same cycle.
  // Only a1 reaches leaf, two steps into the cycle. The digest is pinned in
  // the Rust hasher's tests too, since both CLIs share one store.
  test('cycle members with one package id split by what they reach', () => {
    const depsGraph = {
      a1: node('a', { b: 'b1' }),
      a2: node('a', { b: 'b2' }),
      b1: node('b', { a: 'a2', leaf: 'leaf' }),
      b2: node('b', { a: 'a1' }),
      leaf: node('leaf'),
    }
    expect(graphHash(depsGraph, 'a1')).toBe('OP/WBenoO6ds9rjouLr7e1U1KKHcnH5ZP6fvD1vWIjs=')
    expect(graphHash(depsGraph, 'a1')).not.toBe(graphHash(depsGraph, 'a2'))
    expect(graphHash(depsGraph, 'b1')).not.toBe(graphHash(depsGraph, 'b2'))
  })

  test('cycle members with identical trees share a digest', () => {
    const depsGraph = {
      a1: node('a', { b: 'b1' }),
      b1: node('b', { a: 'a2' }),
      a2: node('a', { b: 'b2' }),
      b2: node('b', { a: 'a1' }),
    }
    expect(graphHash(depsGraph, 'a1')).toBe(graphHash(depsGraph, 'a2'))
    expect(graphHash(depsGraph, 'b1')).toBe(graphHash(depsGraph, 'b2'))
    expect(graphHash(depsGraph, 'a1')).not.toBe(graphHash(depsGraph, 'b1'))
  })

  test('a repeated package id without a cycle is not truncated', () => {
    const withLeaf = (leafId: string) => ({
      a1: node('a', { b: 'b' }),
      b: node('b', { a: 'a2' }),
      a2: node('a', { leaf: 'leaf' }),
      leaf: node(leafId),
    })
    expect(graphHash(withLeaf('leaf-1'), 'a1')).not.toBe(graphHash(withLeaf('leaf-2'), 'a1'))
  })
})
