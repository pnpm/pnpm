import { parentIdsContainSequence } from '../lib/parentIdsContainSequence'

test('parentIdsContainSequence()', () => {
  expect(parentIdsContainSequence(['.', 'b', 'a', 'c', 'b', 'a'], 'a', 'b')).toBeTruthy()
})
