import { calcDepState } from '@pnpm/calc-dep-state'
import { ENGINE_NAME } from '@pnpm/constants'
import { hashObject } from '@pnpm/crypto.object-hasher'

const depsGraph = {
  'registry/foo@1.0.0': {
    depPath: '/foo@1.0.0',
    children: {
      bar: 'registry/bar@1.0.0',
    },
  },
  'registry/bar@1.0.0': {
    depPath: '/bar@1.0.0',
    children: {
      foo: 'registry/foo@1.0.0',
    },
  },
}

test('calcDepState()', () => {
  expect(calcDepState(depsGraph, {}, '/registry/foo@1.0.0', {
    isBuilt: true,
  })).toBe(`${ENGINE_NAME}-${hashObject({})}`)
})

test('calcDepState() when scripts are ignored', () => {
  expect(calcDepState(depsGraph, {}, '/registry/foo@1.0.0', {
    isBuilt: false,
  })).toBe(ENGINE_NAME)
})
