import { calcDepState } from '@pnpm/calc-dep-state'
import { ENGINE_NAME } from '@pnpm/constants'

test('calcDepState()', () => {
  expect(calcDepState('/registry/foo/1.0.0', {
    'registry/foo/1.0.0': {
      depPath: '/foo/1.0.0',
      children: {
        bar: 'registry/bar/1.0.0',
      },
    },
    'registry/bar/1.0.0': {
      depPath: '/bar/1.0.0',
      children: {
        foo: 'registry/foo/1.0.0',
      },
    },
  }, {})).toBe(`${ENGINE_NAME}-{}`)
})
