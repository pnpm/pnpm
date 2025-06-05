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
          integrity: '0',
        },
      },
      ['bar@1.0.0' as DepPath]: {
        dependencies: {
          qar: '1.0.0',
        },
        resolution: {
          integrity: '1',
        },
      },
      ['qar@1.0.0' as DepPath]: {
        resolution: {
          integrity: '2',
        },
      },
    },
  })).toStrictEqual({
    'bar@1.0.0': {
      children: {
        qar: 'qar@1.0.0',
      },
      uniquePkgId: 'bar@1.0.0/1',
    },
    'foo@1.0.0': {
      children: {
        bar: 'bar@1.0.0',
        qar: 'qar@1.0.0',
      },
      uniquePkgId: 'foo@1.0.0/0',
    },
    'qar@1.0.0': {
      children: {},
      uniquePkgId: 'qar@1.0.0/2',
    },
  })
})
