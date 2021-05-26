import { node } from '@pnpm/plugin-commands-nvm'

test('check API (placeholder test)', async () => {
  expect(typeof node.getNodeDir).toBe('function')
})
