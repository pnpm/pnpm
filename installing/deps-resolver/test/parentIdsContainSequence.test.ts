import type { PkgResolutionId } from '@pnpm/types'

import { parentIdsContainSequence } from '../lib/parentIdsContainSequence.js'
import { expect, test } from '@jest/globals'

test('parentIdsContainSequence()', () => {
  expect(parentIdsContainSequence(['.', 'b', 'a', 'c', 'b', 'a'] as PkgResolutionId[], 'a' as PkgResolutionId, 'b' as PkgResolutionId)).toBeTruthy()
})
