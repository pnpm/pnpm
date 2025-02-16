import { sortKeysByPriority } from '@pnpm/object.key-sorting'

test('sortKeysByPriority', () => {
  expect(Object.keys(sortKeysByPriority({
    priority: {
      foo: 1,
      bar: 2,
      qar: 3,
    },
  }, {
    a: 'a',
    qar: 'qar',
    b: 'b',
    foo: 'foo',
    c: 'c',
    bar: 'bar',
  }))).toStrictEqual(['foo', 'bar', 'qar', 'a', 'b', 'c'])
})
