import { expect, test } from '@jest/globals'
import { addDependenciesToPackage } from '@pnpm/installing.deps-installer'
import { streamParser } from '@pnpm/logger'
import { prepareEmpty } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/testing.registry-mock'

import { testDefaults } from '../utils/index.js'

test('convergence override rewrites only the edges its version satisfies', async () => {
  const project = prepareEmpty()

  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  const overrides = {
    '@pnpm.e2e/dep-of-pkg-with-1-dep@': '100.0.0',
  }
  await addDependenciesToPackage({},
    ['@pnpm.e2e/pkg-with-1-dep@100.0.0', '@pnpm.e2e/dep-of-pkg-with-1-dep@101.0.0'],
    testDefaults({ overrides })
  )

  const lockfile = project.readLockfile()
  // The transitive edge (declared as ^100.0.0) is governed: it converges on
  // 100.0.0 instead of resolving to the latest 100.1.0.
  expect(lockfile.snapshots['@pnpm.e2e/pkg-with-1-dep@100.0.0'].dependencies?.['@pnpm.e2e/dep-of-pkg-with-1-dep']).toBe('100.0.0')
  // The direct dependency pinned to an incompatible version keeps its own
  // resolution.
  expect(lockfile.packages).toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@101.0.0'])
  expect(lockfile.overrides).toStrictEqual({
    '@pnpm.e2e/dep-of-pkg-with-1-dep@': '100.0.0',
  })
})

test('a stale convergence override is reported with the version every declared range admits', async () => {
  prepareEmpty()

  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  const overrides = {
    '@pnpm.e2e/dep-of-pkg-with-1-dep@': '100.0.0',
  }
  const warnings: string[] = []
  const reporter = (log: { level?: string, message?: string }) => {
    if (log.level === 'warn' && log.message != null) warnings.push(log.message)
  }
  streamParser.on('data', reporter as never)
  try {
    // The only declared range is ^100.0.0 (from pkg-with-1-dep), which also
    // admits the latest 100.1.0, so the override is stale.
    await addDependenciesToPackage({},
      ['@pnpm.e2e/pkg-with-1-dep@100.0.0'],
      testDefaults({ overrides })
    )
  } finally {
    streamParser.removeListener('data', reporter as never)
  }

  expect(warnings).toContainEqual(expect.stringContaining('The convergence override "@pnpm.e2e/dep-of-pkg-with-1-dep@": "100.0.0" is stale'))
  expect(warnings).toContainEqual(expect.stringContaining('Change the override\'s value to 100.1.0'))
})
