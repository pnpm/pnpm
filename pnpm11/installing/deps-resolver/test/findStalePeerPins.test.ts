import { expect, test } from '@jest/globals'
import { EXISTING_VERSION_SELECTOR_WEIGHT, type PreferredVersions } from '@pnpm/resolving.resolver-base'

import { releaseStalePeerPins } from '../lib/findStalePeerPins.js'

test.each(['1.0.0', 'v1.0.0', '=1.0.0', ' =v1.0.0 '])('releasing a pin removes the lockfile weight from the version that %p names', (spec) => {
  const lockfileWeighted = { selectorType: 'version' as const, weight: EXISTING_VERSION_SELECTOR_WEIGHT }
  const preferredVersions: PreferredVersions = {
    foo: { '1.0.0': lockfileWeighted, '2.0.0': lockfileWeighted, '3.0.0': lockfileWeighted },
  }
  const released = releaseStalePeerPins(new Map([['foo', '2.0.0']]), {
    directSpecsByName: new Map([['foo', new Set([spec])]]),
    preferredVersions,
    resolvedDependencies: { foo: '2.0.0' },
  })
  expect({ ...released.preferredVersions.foo }).toStrictEqual({ '3.0.0': lockfileWeighted })
  expect(released.resolvedDependencies).toStrictEqual({})
})
