import { beforeEach, expect, jest, test } from '@jest/globals'
import type { BlockedVersions } from '@pnpm/resolving.resolver-base'

import type { ResolveDependenciesOptions, ResolveDependencyTreeResult } from '../lib/resolveDependencyTree.js'

const resolveDependencyTree = jest.fn<(importers: unknown[], opts: ResolveDependenciesOptions) => Promise<ResolveDependencyTreeResult>>()
jest.unstable_mockModule('../lib/resolveDependencyTree.js', () => ({ resolveDependencyTree }))
const logger = await import('@pnpm/logger')
const globalInfo = jest.fn()
jest.unstable_mockModule('@pnpm/logger', () => ({ ...logger, globalInfo }))
const { resolveMatureDependencyTree } = await import('../lib/resolveMatureDependencyTree.js')

beforeEach(() => {
  resolveDependencyTree.mockReset()
  globalInfo.mockClear()
})

test('the retry retains the registry identity of the blamed parent', async () => {
  const first = tree(['parent@work:2.0.0'])
  resolveDependencyTree.mockResolvedValueOnce(first).mockImplementationOnce(async (_, opts) => {
    expect(opts.blockedVersions).toEqual(new Map([['parent', new Set(['work:2.0.0'])]]))
    return tree([])
  })
  await resolveMatureDependencyTree(async () => [], { minimumReleaseAge: 1440 } as ResolveDependenciesOptions)
  expect(resolveDependencyTree).toHaveBeenCalledTimes(2)
})

test('an exotic parent cannot be blocked as a registry version', async () => {
  const first = tree(['parent@file:../parent'])
  resolveDependencyTree.mockResolvedValue(first)
  const result = await resolveMatureDependencyTree(async () => [], { minimumReleaseAge: 1440 } as ResolveDependenciesOptions)
  expect(result.tree).toBe(first)
  expect(resolveDependencyTree).toHaveBeenCalledTimes(1)
})

test('held-back packages are reported in package-name order', async () => {
  resolveDependencyTree.mockResolvedValueOnce(tree(['z-parent@2.0.0', 'a-parent@2.0.0'])).mockResolvedValueOnce(tree([]))
  await resolveMatureDependencyTree(async () => [], { minimumReleaseAge: 1440 } as ResolveDependenciesOptions)
  expect(globalInfo).toHaveBeenCalledWith(expect.stringContaining('\n  a-parent@2.0.0\n  z-parent@2.0.0'))
})

test('failed retries return the original violations', async () => {
  const first = tree(['parent@2.0.0'])
  const retry = tree(['parent@1.0.0'])
  let blocks: BlockedVersions | undefined
  resolveDependencyTree.mockResolvedValueOnce(first).mockImplementation(async (_, opts) => {
    blocks = opts.blockedVersions
    return retry
  })
  const result = await resolveMatureDependencyTree(async () => [], { minimumReleaseAge: 1440 } as ResolveDependenciesOptions)
  expect(blocks?.get('parent')).toEqual(new Set(['2.0.0', '1.0.0']))
  expect(result.tree).toBe(first)
})

function tree (parentIds: string[]): ResolveDependencyTreeResult {
  return {
    resolutionPolicyViolations: parentIds.map(id => ({
      code: 'MINIMUM_RELEASE_AGE_VIOLATION', name: 'child', version: '1.0.0', reason: 'too new', parentIds: [id],
    })),
    resolvedPkgsById: Object.fromEntries(parentIds.map(id => [id, { id, name: id.split('@')[0], version: '2.0.0' }])),
  } as unknown as ResolveDependencyTreeResult
}

test('a mature retry keeps other policy violations for their own handlers', async () => {
  const trust = {
    code: 'TRUST_DOWNGRADE', name: 'other', version: '1.0.0', reason: 'trust downgrade',
    resolution: { tarball: 'https://registry.example/other.tgz' },
  }
  const first = tree(['parent@2.0.0'])
  first.resolutionPolicyViolations.push(trust)
  const retry = tree([])
  retry.resolutionPolicyViolations.push(trust)
  resolveDependencyTree.mockResolvedValueOnce(first).mockResolvedValueOnce(retry)
  const result = await resolveMatureDependencyTree(async () => [], { minimumReleaseAge: 1440 } as ResolveDependenciesOptions)
  expect(result.tree).toBe(retry)
  expect(result.tree.resolutionPolicyViolations).toEqual([trust])
  expect(resolveDependencyTree).toHaveBeenCalledTimes(2)
})
