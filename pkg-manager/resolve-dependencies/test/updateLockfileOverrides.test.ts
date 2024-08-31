import { type ProjectSnapshot } from '@pnpm/lockfile.types'
import { updateLockfileOverrides } from '../src/updateLockfileOverrides'

const existingOverrides = (): Record<string, string> => ({
  foo: '^0.1.0',
  bar: '~1.2.0',
  abc: '2.0.0',
  def: '3.0.0',
})

const rootSnapshot = (): ProjectSnapshot => ({
  specifiers: {
    foo: '^0.1.2',
    bar: '~1.2.3',
    baz: '>=2.0.0',
    qux: '3.3.3',
  },
})

test('returns undefined when there is no existing overrides', () => {
  expect(updateLockfileOverrides(undefined, rootSnapshot(), {})).toBeUndefined()
})

test('returns the input overrides when there is no root importer', () => {
  expect(updateLockfileOverrides(existingOverrides(), undefined, {})).toStrictEqual(existingOverrides())
})

test('updates all overrides when there are all references', () => {
  expect(updateLockfileOverrides(
    existingOverrides(),
    rootSnapshot(),
    {
      foo: 'foo',
      bar: 'bar',
      abc: 'baz',
      def: 'qux',
    }
  )).toStrictEqual({
    foo: rootSnapshot().specifiers.foo,
    bar: rootSnapshot().specifiers.bar,
    abc: rootSnapshot().specifiers.baz,
    def: rootSnapshot().specifiers.qux,
  })
})

test('skips undefined references', () => {
  expect(updateLockfileOverrides(
    existingOverrides(),
    rootSnapshot(),
    {
      foo: 'foo',
      bar: undefined,
      abc: 'baz',
      def: undefined,
    }
  )).toStrictEqual({
    foo: rootSnapshot().specifiers.foo,
    bar: existingOverrides().bar,
    abc: rootSnapshot().specifiers.baz,
    def: existingOverrides().def,
  })
})
