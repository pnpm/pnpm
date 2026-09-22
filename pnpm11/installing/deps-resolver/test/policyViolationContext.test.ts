import { expect, test } from '@jest/globals'
import type { PkgResolutionId } from '@pnpm/types'

import { getPolicyViolationContext, type ResolvedPkgsById } from '../lib/resolveDependencies.js'

test.each([0, 1, 32, 33, 1024])('bounds diagnostic ancestry at depth %i while retaining the retry parent', (depth) => {
  const ids = Array.from({ length: depth }, (_, index) => `parent-${index}@work:1.0.0` as PkgResolutionId)
  const packages = Object.fromEntries(ids.map((id, index) => [id, { name: `parent-${index}`, version: '1.0.0' }])) as ResolvedPkgsById
  const context = getPolicyViolationContext(['importer' as PkgResolutionId, ...ids], packages)
  expect(context.retryParentId).toBe(ids.at(-1))
  expect(context.parentsTruncated).toBe(depth > 32)
  expect(context.parents).toEqual(ids.slice(-32).map((id) => ({ name: id.split('@')[0], version: 'work:1.0.0' })))
})
