import { lockfileToDepGraph } from '@pnpm/calc-dep-state'

test('lockfileToDepGraph', () => {
  expect(lockfileToDepGraph({
    lockfileVersion: '7.0',
    importers: {},
    packages: {
      'foo@1.0.0': {
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
      'bar@1.0.0': {
        dependencies: {
          qar: '1.0.0',
        },
        resolution: {
          integrity: '',
        },
      },
      'qar@1.0.0': {
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
      depPath: 'bar@1.0.0',
    },
    'foo@1.0.0': {
      children: {
        bar: 'bar@1.0.0',
        qar: 'qar@1.0.0',
      },
      depPath: 'foo@1.0.0',
    },
    'qar@1.0.0': {
      children: {},
      depPath: 'qar@1.0.0',
    },
  })
})
