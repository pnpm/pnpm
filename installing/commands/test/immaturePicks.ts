import { expect, jest, test } from '@jest/globals'

import { createImmaturePickCollector, drainImmaturePicks } from '../lib/immaturePicks.js'

test('createImmaturePickCollector returns undefined when minimumReleaseAge is unset', () => {
  expect(createImmaturePickCollector({})).toBeUndefined()
})

test('createImmaturePickCollector returns undefined when strict mode is on', () => {
  // Strict mode means the resolver throws on immature picks rather than
  // falling back, so there's nothing to collect — keep the collector inert
  // so opts.onImmaturePick passes `undefined` through and the resolver
  // can short-circuit the notify path.
  expect(createImmaturePickCollector({ minimumReleaseAge: 60, minimumReleaseAgeStrict: true })).toBeUndefined()
})

test('createImmaturePickCollector records and dedupes name@version entries', () => {
  const collector = createImmaturePickCollector({ minimumReleaseAge: 60 })!
  collector.record({ name: 'foo', version: '1.0.0' })
  collector.record({ name: 'foo', version: '1.0.0' })
  collector.record({ name: 'bar', version: '2.3.4' })

  expect(collector.versions).toEqual(new Set(['foo@1.0.0', 'bar@2.3.4']))
})

test('drainImmaturePicks returns sorted entries, clears the set, and logs once', () => {
  const collector = createImmaturePickCollector({ minimumReleaseAge: 60 })!
  collector.record({ name: 'foo', version: '1.0.0' })
  collector.record({ name: 'bar', version: '2.3.4' })

  // Avoid leaking console output in test runs.
  const infoSpy = jest.spyOn(console, 'log').mockImplementation(() => {})
  try {
    const drained = drainImmaturePicks(collector)
    expect(drained).toEqual(['bar@2.3.4', 'foo@1.0.0'])
  } finally {
    infoSpy.mockRestore()
  }

  // Subsequent drain returns nothing — important so a follow-up install in
  // the same process doesn't re-announce entries already persisted.
  expect(drainImmaturePicks(collector)).toBeUndefined()
})

test('drainImmaturePicks returns undefined for an empty or absent collector', () => {
  expect(drainImmaturePicks(undefined)).toBeUndefined()

  const empty = createImmaturePickCollector({ minimumReleaseAge: 60 })!
  expect(drainImmaturePicks(empty)).toBeUndefined()
})
