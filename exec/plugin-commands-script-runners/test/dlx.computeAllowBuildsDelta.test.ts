import { computeAllowBuildsDelta } from '../src/dlx.js'

test('returns none when both are empty', () => {
  expect(computeAllowBuildsDelta({}, {})).toEqual({ action: 'none', newlyAllowed: [] })
})

test('returns none when current and cached are identical', () => {
  const builds = { foo: true as const, bar: true as const }
  expect(computeAllowBuildsDelta(builds, builds)).toEqual({ action: 'none', newlyAllowed: [] })
})

test('returns rebuild when current has new packages', () => {
  const cached = { foo: true as const }
  const current = { foo: true as const, bar: true as const }
  const result = computeAllowBuildsDelta(current, cached)
  expect(result.action).toBe('rebuild')
  expect(result.newlyAllowed).toEqual(['bar'])
})

test('returns rebuild with multiple newly allowed packages', () => {
  const cached = { foo: true as const }
  const current = { foo: true as const, bar: true as const, baz: true as const }
  const result = computeAllowBuildsDelta(current, cached)
  expect(result.action).toBe('rebuild')
  expect(result.newlyAllowed.sort()).toEqual(['bar', 'baz'])
})

test('returns invalidate when a previously allowed package is removed', () => {
  const cached = { foo: true as const, bar: true as const }
  const current = { foo: true as const }
  expect(computeAllowBuildsDelta(current, cached)).toEqual({ action: 'invalidate', newlyAllowed: [] })
})

test('returns invalidate even if new packages are also added', () => {
  // If a package was removed and another added, invalidation takes priority
  const cached = { foo: true as const, bar: true as const }
  const current = { foo: true as const, baz: true as const }
  expect(computeAllowBuildsDelta(current, cached).action).toBe('invalidate')
})

test('returns none when current is empty and cached is empty', () => {
  expect(computeAllowBuildsDelta({}, {})).toEqual({ action: 'none', newlyAllowed: [] })
})

test('returns rebuild when going from empty cached to non-empty current', () => {
  const result = computeAllowBuildsDelta({ foo: true as const }, {})
  expect(result.action).toBe('rebuild')
  expect(result.newlyAllowed).toEqual(['foo'])
})

test('returns invalidate when going from non-empty cached to empty current', () => {
  expect(computeAllowBuildsDelta({}, { foo: true as const })).toEqual({ action: 'invalidate', newlyAllowed: [] })
})
