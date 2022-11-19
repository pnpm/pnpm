import { nodeIdContainsSequence } from '../lib/nodeIdUtils'

test('nodeIdContainsSequence()', () => {
  expect(nodeIdContainsSequence('>.>b>a>c>b>a>', 'a', 'b')).toBeTruthy()
})
