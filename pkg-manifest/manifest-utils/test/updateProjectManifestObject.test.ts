import { guessDependencyType } from '@pnpm/manifest-utils'

test('guessDependencyType()', () => {
  expect(
    guessDependencyType('foo', {
      dependencies: {
        bar: '1.0.0',
      },
      devDependencies: {
        foo: '',
      },
    })
  ).toBe('devDependencies')

  expect(
    guessDependencyType('bar', {
      dependencies: {
        bar: '1.0.0',
      },
      devDependencies: {
        foo: '1.0.0',
      },
    })
  ).toBe('dependencies')
})
