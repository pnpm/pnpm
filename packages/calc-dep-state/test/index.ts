import { calcDepState } from '@pnpm/calc-dep-state'
import { ENGINE_NAME } from '@pnpm/constants'
import { hashObject } from '@pnpm/crypto.object-hasher'
import { type PkgIdWithPatchHash } from '@pnpm/types'

const depsGraph = {
  'foo@1.0.0': {
    pkgIdWithPatchHash: 'foo@1.0.0' as PkgIdWithPatchHash,
    resolution: {
      integrity: '000',
    },
    children: {
      bar: 'bar@1.0.0',
    },
  },
  'bar@1.0.0': {
    pkgIdWithPatchHash: 'bar@1.0.0' as PkgIdWithPatchHash,
    resolution: {
      integrity: '001',
    },
    children: {
      foo: 'foo@1.0.0',
    },
  },
}

test('calcDepState()', () => {
  expect(calcDepState(depsGraph, {}, 'foo@1.0.0', {
    includeDepGraphHash: true,
  })).toBe(`${ENGINE_NAME};deps=${hashObject({
    id: 'foo@1.0.0:000',
    deps: {
      bar: hashObject({
        id: 'bar@1.0.0:001',
        deps: {
          foo: hashObject({
            id: 'foo@1.0.0:000',
            deps: {},
          }),
        },
      }),
    },
  })}`)
})

test('calcDepState() when scripts are ignored', () => {
  expect(calcDepState(depsGraph, {}, 'foo@1.0.0', {
    includeDepGraphHash: false,
  })).toBe(ENGINE_NAME)
})
