import { node } from '@pnpm/plugin-commands-env'

test('check API (placeholder test)', async () => {
  expect(typeof node.getNodeDir).toBe('function')
})
