import { deployHook } from '../src/deployHook'

test('deployHook()', () => {
  expect(deployHook({
    dependencies: {
      a: 'workspace:1',
    },
    devDependencies: {
      b: 'workspace:2',
    },
    optionalDependencies: {
      c: 'workspace:3',
    },
  })).toStrictEqual({
    dependencies: {
      a: 'workspace:1',
    },
    devDependencies: {
      b: 'workspace:2',
    },
    optionalDependencies: {
      c: 'workspace:3',
    },
    dependenciesMeta: {
      a: {
        injected: true,
      },
      b: {
        injected: true,
      },
      c: {
        injected: true,
      },
    },
  })
})
