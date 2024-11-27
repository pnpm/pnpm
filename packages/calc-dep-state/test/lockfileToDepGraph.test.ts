import { lockfileToDepGraph } from '@pnpm/calc-dep-state'
import { type DepPath } from '@pnpm/types'

test('lockfileToDepGraph', () => {
  expect(lockfileToDepGraph({
    lockfileVersion: '9.0',
    importers: {},
    packages: {
      ['foo@1.0.0' as DepPath]: {
        dependencies: {
          bar: '1.0.0',
        },
        optionalDependencies: {
          qar: '1.0.0',
        },
        resolution: {
          integrity: '',
        },
      },
      ['bar@1.0.0' as DepPath]: {
        dependencies: {
          qar: '1.0.0',
        },
        resolution: {
          integrity: '',
        },
      },
      ['qar@1.0.0' as DepPath]: {
        resolution: {
          integrity: '',
        },
      },
    },
  })).toStrictEqual({
    'bar@1.0.0': {
      children: {
        qar: 'qar@1.0.0',
      },
      pkgIdWithPatchHash: 'bar@1.0.0',
    },
    'foo@1.0.0': {
      children: {
        bar: 'bar@1.0.0',
        qar: 'qar@1.0.0',
      },
      pkgIdWithPatchHash: 'foo@1.0.0',
    },
    'qar@1.0.0': {
      children: {},
      pkgIdWithPatchHash: 'qar@1.0.0',
    },
  })
})
