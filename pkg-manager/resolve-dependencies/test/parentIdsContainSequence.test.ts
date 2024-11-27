import { type PkgResolutionId } from '@pnpm/types'
import { parentIdsContainSequence } from '../lib/parentIdsContainSequence'

test('parentIdsContainSequence()', () => {
  expect(parentIdsContainSequence(['.', 'b', 'a', 'c', 'b', 'a'] as PkgResolutionId[], 'a' as PkgResolutionId, 'b' as PkgResolutionId)).toBeTruthy()
})
