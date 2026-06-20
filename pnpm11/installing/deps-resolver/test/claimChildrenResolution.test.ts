import { expect, test } from '@jest/globals'
import type { PkgResolutionId } from '@pnpm/resolving.resolver-base'

import { claimChildrenResolution } from '../lib/resolveDependencies.js'

type Ctx = Parameters<typeof claimChildrenResolution>[0]

function createCtx (importerResolutionOrder: Record<string, number> = {}): Ctx {
  return {
    hoistPeers: true,
    importerResolutionOrder,
    childrenResolutionByPkgId: {},
    childrenResolutionId: 0,
    missingPeersOfChildrenByPkgId: {},
  } as unknown as Ctx
}

// Regression test for https://github.com/pnpm/pnpm/pull/12514.
//
// When several occurrences of one shared package each resolve its children, the
// shallowest occurrence becomes the "owner" and the others may reuse the owner's
// `missingPeersOfChildren` promise. Reuse must be a function of the dependency
// graph (the owner's depth), never of whether the owner's promise happens to
// have settled by the time a given occurrence is claimed: that settling time
// varies run to run under concurrent resolution, so keying reuse on it flipped a
// transitive optional peer (e.g. styled-jsx's `@babel/core`) in and out of a
// deeper consumer's resolved suffix and churned the lockfile.
test('a deeper consumer never inherits a shallower owner\'s missing peers, even after the owner has resolved', () => {
  const ctx = createCtx()
  const pkgId = 'shared@1.0.0' as PkgResolutionId

  const owner = claimChildrenResolution(ctx, {
    currentDepth: 0,
    parentIds: ['importer-1'] as PkgResolutionId[],
    pkgId,
  })
  expect(owner.isOwner).toBe(true)
  expect(owner.missingPeersOfChildren).toBeDefined()

  // Simulate the owner's subtree finishing before the deeper consumer is
  // claimed. This settled state is the only run-to-run-variable input the buggy
  // reuse condition keyed on.
  ctx.childrenResolutionByPkgId[pkgId].missingPeersOfChildren!.resolved = true

  const deeperConsumer = claimChildrenResolution(ctx, {
    currentDepth: 2,
    parentIds: ['importer-1', 'a@1.0.0', 'b@1.0.0'] as PkgResolutionId[],
    pkgId,
  })
  expect(deeperConsumer.isOwner).toBe(false)
  // The owner is strictly shallower, so the consumer resolves its own children's
  // peers regardless of the settled flag.
  expect(deeperConsumer.missingPeersOfChildren).toBeUndefined()
})

test('a same-depth occurrence still reuses the owner\'s missing peers', () => {
  const ctx = createCtx({ 'importer-1': 0, 'importer-2': 1 })
  const pkgId = 'shared@1.0.0' as PkgResolutionId

  // The first importer wins ownership on the importer-order tiebreak.
  const owner = claimChildrenResolution(ctx, {
    currentDepth: 1,
    parentIds: ['importer-1', 'mid-a@1.0.0'] as PkgResolutionId[],
    pkgId,
  })
  expect(owner.isOwner).toBe(true)
  expect(owner.missingPeersOfChildren).toBeDefined()

  // A second occurrence at the same depth shares the owner's promise. This reuse
  // is structural — both occurrences are equidistant — and is intentionally kept.
  const sibling = claimChildrenResolution(ctx, {
    currentDepth: 1,
    parentIds: ['importer-2', 'mid-b@1.0.0'] as PkgResolutionId[],
    pkgId,
  })
  expect(sibling.isOwner).toBe(false)
  expect(sibling.missingPeersOfChildren).toBe(owner.missingPeersOfChildren)
})
