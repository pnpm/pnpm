import { createObjectChecksum } from './index'

function assets () {
  const sorted = {
    abc: {
      a: 0,
      b: [0, 1, 2],
      c: null,
    },
    def: {
      foo: 'bar',
      hello: 'world',
    },
  } as const

  const unsorted1 = {
    abc: {
      b: [0, 1, 2],
      a: 0,
      c: null,
    },
    def: {
      hello: 'world',
      foo: 'bar',
    },
  } as const

  const unsorted2 = {
    def: {
      foo: 'bar',
      hello: 'world',
    },
    abc: {
      a: 0,
      b: [0, 1, 2],
      c: null,
    },
  } as const

  const unsorted3 = {
    def: {
      hello: 'world',
      foo: 'bar',
    },
    abc: {
      b: [0, 1, 2],
      a: 0,
      c: null,
    },
  } as const

  return { sorted, unsorted1, unsorted2, unsorted3 } as const
}

test('createObjectChecksum', () => {
  const { sorted, unsorted1, unsorted2, unsorted3 } = assets()
  expect(createObjectChecksum(unsorted1)).toBe(createObjectChecksum(sorted))
  expect(createObjectChecksum(unsorted2)).toBe(createObjectChecksum(sorted))
  expect(createObjectChecksum(unsorted3)).toBe(createObjectChecksum(sorted))
})
