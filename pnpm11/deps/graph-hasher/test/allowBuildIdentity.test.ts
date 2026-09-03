import { expect, it } from '@jest/globals'
import { type DepsGraph, iterateHashedGraphNodes } from '@pnpm/deps.graph-hasher'
import type { AllowBuild, DepPath } from '@pnpm/types'

it('gates built dep paths through the allowBuild policy by depPath', () => {
  const registryDepPath = 'foo@1.0.0' as DepPath
  const directTarballDepPath = 'foo@https://example.com/foo.tgz' as DepPath
  const checkedDepPaths: DepPath[] = []
  const allowBuild: AllowBuild = (depPath) => {
    checkedDepPaths.push(depPath)
    return depPath === registryDepPath
  }

  Array.from(iterateHashedGraphNodes(
    {
      [registryDepPath]: {
        children: {},
        fullPkgId: 'foo@1.0.0:sha512-abc',
      },
      [directTarballDepPath]: {
        children: {},
        fullPkgId: 'foo@1.0.0:sha512-def',
      },
    },
    [
      {
        depPath: registryDepPath,
        name: 'foo',
        version: '1.0.0',
      },
      {
        depPath: directTarballDepPath,
        name: 'foo',
        version: '1.0.0',
      },
    ].values(),
    { allowBuild }
  ))

  expect(checkedDepPaths).toStrictEqual([registryDepPath, directTarballDepPath])
})

it('includes the engine for every cycle member that reaches a builder', () => {
  const a = 'a@1.0.0' as DepPath
  const b = 'b@1.0.0' as DepPath
  const builder = 'builder@1.0.0' as DepPath
  const pureJs = 'pure-js@1.0.0' as DepPath
  const graph: DepsGraph<DepPath> = {
    [a]: {
      children: { b, builder },
      fullPkgId: 'a@1.0.0:sha512-a',
    },
    [b]: {
      children: { a },
      fullPkgId: 'b@1.0.0:sha512-b',
    },
    [builder]: {
      children: {},
      fullPkgId: 'builder@1.0.0:sha512-builder',
    },
    [pureJs]: {
      children: {},
      fullPkgId: 'pure-js@1.0.0:sha512-pure',
    },
  }
  const pkgMeta = [
    { depPath: a, name: 'a', version: '1.0.0' },
    { depPath: b, name: 'b', version: '1.0.0' },
    { depPath: builder, name: 'builder', version: '1.0.0' },
    { depPath: pureJs, name: 'pure-js', version: '1.0.0' },
  ]
  const reversePkgMeta = [pkgMeta[1], pkgMeta[0], pkgMeta[2], pkgMeta[3]]

  for (const orderedPkgMeta of [pkgMeta, reversePkgMeta]) {
    const node20 = hashesFor('20.0.0', orderedPkgMeta)
    const node22 = hashesFor('22.0.0', orderedPkgMeta)

    expect(node20.get(a)).not.toBe(node22.get(a))
    expect(node20.get(b)).not.toBe(node22.get(b))
    expect(node20.get(pureJs)).toBe(node22.get(pureJs))
  }

  function hashesFor (nodeVersion: string, orderedPkgMeta: typeof pkgMeta): Map<DepPath, string> {
    return new Map(
      Array.from(iterateHashedGraphNodes(graph, orderedPkgMeta.values(), {
        allowBuild: (depPath) => depPath === builder,
        nodeVersion,
      })).map(({ hash, pkgMeta }) => [pkgMeta.depPath, hash])
    )
  }
})
