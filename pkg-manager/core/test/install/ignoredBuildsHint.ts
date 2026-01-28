import { type DepPath } from '@pnpm/types'
import { IgnoredBuildsError } from '../../src/install/index.js'

test('ignored builds hint includes -g for global installs', () => {
  const ignoredBuilds = new Set<DepPath>(['esbuild@0.27.2' as DepPath])
  const error = new IgnoredBuildsError(ignoredBuilds, { global: true })

  expect(error.hint).toBe(
    'Run "pnpm approve-builds -g" to pick which dependencies should be allowed to run scripts.'
  )
})

test('ignored builds hint omits -g for non-global installs', () => {
  const ignoredBuilds = new Set<DepPath>(['esbuild@0.27.2' as DepPath])
  const error = new IgnoredBuildsError(ignoredBuilds)

  expect(error.hint).toBe(
    'Run "pnpm approve-builds" to pick which dependencies should be allowed to run scripts.'
  )
})
