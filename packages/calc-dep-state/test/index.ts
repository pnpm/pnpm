import { calcDepState } from '@pnpm/calc-dep-state'
import { ENGINE_NAME } from '@pnpm/constants'
import { hashObject } from '@pnpm/crypto.object-hasher'
import { type PkgId } from '@pnpm/types'

const depsGraph = {
  'registry/foo@1.0.0': {
    packageId: 'foo@1.0.0' as PkgId,
    children: {
      bar: 'registry/bar@1.0.0',
    },
  },
  'registry/bar@1.0.0': {
    packageId: 'bar@1.0.0' as PkgId,
    children: {
      foo: 'registry/foo@1.0.0',
    },
  },
}

test('calcDepState()', () => {
  expect(calcDepState(depsGraph, {}, 'registry/foo@1.0.0', {
    isBuilt: true,
  })).toBe(`${ENGINE_NAME}-${hashObject({
    'bar@1.0.0': { 'foo@1.0.0': {} },
  })}`)
})

test('calcDepState() when scripts are ignored', () => {
  expect(calcDepState(depsGraph, {}, 'registry/foo@1.0.0', {
    isBuilt: false,
  })).toBe(ENGINE_NAME)
})
