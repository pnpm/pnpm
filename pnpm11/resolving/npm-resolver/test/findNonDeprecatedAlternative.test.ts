import { expect, test } from '@jest/globals'
import type { PackageMeta } from '@pnpm/resolving.registry.types'

import { findNonDeprecatedAlternative } from '../src/pickPackageFromMeta.js'

function metaWith (versions: Record<string, boolean>): PackageMeta {
  return {
    name: 'foo',
    'dist-tags': {},
    versions: Object.fromEntries(
      Object.entries(versions).map(([version, deprecated]) => [
        version,
        { name: 'foo', version, ...(deprecated ? { deprecated: 'do not use' } : {}) },
      ])
    ),
  } as unknown as PackageMeta
}

test('picks the newest version that is not deprecated', () => {
  const meta = metaWith({ '1.0.0': true, '1.4.0': false, '2.3.1': false, '2.4.0': true })
  expect(findNonDeprecatedAlternative(meta, '^1.0.0')?.version).toBe('2.3.1')
})

test('reports whether the alternative is reachable without widening the range', () => {
  const meta = metaWith({ '1.0.0': true, '1.4.0': false, '2.3.1': true })
  expect(findNonDeprecatedAlternative(meta, '^1.0.0')).toEqual({ version: '1.4.0', satisfiesWanted: true })
  expect(findNonDeprecatedAlternative(meta, '^0.9.0')).toEqual({ version: '1.4.0', satisfiesWanted: false })
})

test('is undefined when every published version is deprecated', () => {
  expect(findNonDeprecatedAlternative(metaWith({ '1.0.0': true, '2.0.0': true }), '*')).toBeUndefined()
})

// A registry can put anything in `versions`, and an unparseable key must not
// become the version pnpm tells the user to move to.
test('skips keys that are not versions', () => {
  const meta = metaWith({ '1.0.0': false, 'not-a-version': false })
  expect(findNonDeprecatedAlternative(meta, '*')?.version).toBe('1.0.0')
})
