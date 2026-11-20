import { expect, test } from '@jest/globals'
import { DIRECT_DEP_SELECTOR_WEIGHT, EXISTING_VERSION_SELECTOR_WEIGHT, RESOLVED_VERSION_SELECTOR_WEIGHT } from '@pnpm/resolving.resolver-base'

import { preferredVersionSelectorsFor } from '../src/preferredVersionSelectors.js'

test('an update target drops the lockfile pins but keeps an audit penalty on the same version', () => {
  const penalty = -(DIRECT_DEP_SELECTOR_WEIGHT + 100) + RESOLVED_VERSION_SELECTOR_WEIGHT
  const selectors = preferredVersionSelectorsFor({
    updateRequested: true,
    preferredVersions: {
      foo: {
        '1.0.0': { selectorType: 'version', weight: EXISTING_VERSION_SELECTOR_WEIGHT },
        '1.0.3': { selectorType: 'version', weight: penalty },
      },
    },
  }, 'foo')

  expect({ ...selectors }).toStrictEqual({ '1.0.3': { selectorType: 'version', weight: penalty } })
})
