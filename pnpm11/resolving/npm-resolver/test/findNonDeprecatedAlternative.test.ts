import { expect, test } from '@jest/globals'
import type { PackageMeta } from '@pnpm/resolving.registry.types'

import type { RegistryPackageSpec } from '../src/parseBareSpecifier.js'
import { findNonDeprecatedAlternative } from '../src/pickPackageFromMeta.js'

function metaWith (versions: Record<string, boolean>, time?: Record<string, string>): PackageMeta {
  return {
    name: 'foo',
    'dist-tags': {},
    ...(time ? { time } : {}),
    versions: Object.fromEntries(
      Object.entries(versions).map(([version, deprecated]) => [
        version,
        { name: 'foo', version, ...(deprecated ? { deprecated: 'do not use' } : {}) },
      ])
    ),
  } as unknown as PackageMeta
}

function range (fetchSpec: string): RegistryPackageSpec {
  return { type: 'range', name: 'foo', fetchSpec }
}

test('picks the newest version that is not deprecated', () => {
  const meta = metaWith({ '1.0.0': true, '1.4.0': false, '2.3.1': false, '2.4.0': true })
  expect(findNonDeprecatedAlternative(meta, range('^1.0.0'), {})?.version).toBe('2.3.1')
})

test('reports whether reaching the alternative means widening the range', () => {
  const meta = metaWith({ '1.0.0': true, '1.4.0': false, '2.3.1': true })
  expect(findNonDeprecatedAlternative(meta, range('^1.0.0'), {})).toEqual({ version: '1.4.0', outsideDeclaredRange: false })
  expect(findNonDeprecatedAlternative(meta, range('^0.9.0'), {})).toEqual({ version: '1.4.0', outsideDeclaredRange: true })
})

// A tag says nothing about which versions are acceptable, so the warning must
// not claim the alternative falls outside a range the user never declared.
test('never claims a range for a tag dependency', () => {
  const meta = metaWith({ '1.0.0': true, '2.0.0': false })
  const tag: RegistryPackageSpec = { type: 'tag', name: 'foo', fetchSpec: 'latest' }
  expect(findNonDeprecatedAlternative(meta, tag, {})).toEqual({ version: '2.0.0', outsideDeclaredRange: false })
})

// Naming a version the active policy would refuse to install would send the
// user somewhere pnpm will not go.
test('skips versions the publish-date policy would refuse', () => {
  const meta = metaWith(
    { '1.0.0': true, '1.4.0': false, '2.0.0': false },
    { '1.4.0': '2020-01-01T00:00:00.000Z', '2.0.0': '2030-01-01T00:00:00.000Z' }
  )
  const opts = { publishedBy: new Date('2025-01-01T00:00:00.000Z') }
  expect(findNonDeprecatedAlternative(meta, range('^1.0.0'), opts)?.version).toBe('1.4.0')
})

test('is undefined when every admissible version is deprecated', () => {
  expect(findNonDeprecatedAlternative(metaWith({ '1.0.0': true, '2.0.0': true }), range('*'), {})).toBeUndefined()
})

// A registry can put anything in `versions`, and an unparseable key must not
// become the version pnpm tells the user to move to.
test('skips keys that are not versions', () => {
  const meta = metaWith({ '1.0.0': false, 'not-a-version': false })
  expect(findNonDeprecatedAlternative(meta, range('*'), {})?.version).toBe('1.0.0')
})
