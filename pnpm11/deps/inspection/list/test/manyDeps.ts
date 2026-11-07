import { expect, test } from '@jest/globals'
import { list } from '@pnpm/deps.inspection.list'
import { fixtures } from '@pnpm/test-fixtures'

const testFixtures = fixtures(import.meta.dirname)
const fixtureWithManyDeps = testFixtures.find('many-deps')

test('list all deps in a project with many dependencies without failing with an OOM error', async () => {
  const output = await list([fixtureWithManyDeps], {
    checkWantedLockfileOnly: true,
    depth: Infinity,
    lockfileDir: fixtureWithManyDeps,
    virtualStoreDirMaxLength: 120,
    reportAs: 'json',
  })
  const json = JSON.parse(output)
  expect(json).toBeTruthy()

  // Walk the JSON tree and collect dedupe / node stats to verify that
  // deduplication is actually happening — a plain toBeTruthy() would not
  // catch regressions where dedupe metadata disappears.
  let totalNodes = 0
  let dedupedNodes = 0
  function walk (deps: Record<string, JsonTreeNode> | undefined): void {
    if (!deps) return
    for (const key of Object.keys(deps)) {
      totalNodes++
      const node = deps[key]
      if (node.deduped) dedupedNodes++
      walk(node.dependencies)
    }
  }
  for (const project of json) {
    for (const field of ['dependencies', 'devDependencies', 'optionalDependencies']) {
      walk(project[field])
    }
  }

  // The fixture has many transitive deps — deduplication must kick in.
  expect(dedupedNodes).toBeGreaterThan(0)
  // The total materialized node count should be bounded well below the
  // combinatorial explosion that caused the original OOM.  The fixture
  // has ~5000 unique packages; without dedupe, the tree can exceed millions
  // of nodes.  With dedupe the count stays in the low tens of thousands.
  expect(totalNodes).toBeLessThan(100_000)
})

interface JsonTreeNode {
  deduped?: boolean
  dependencies?: Record<string, JsonTreeNode>
}
