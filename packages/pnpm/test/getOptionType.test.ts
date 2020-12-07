import {
  currentTypedWordType,
  getLastOption,
  getOptionCompletions,
} from '../src/getOptionType'

const TYPES = {
  color: ['red', 'blue', Array],
  dev: Boolean,
  'save-dev': Boolean,
  'store-dir': String,
}

const SHORTHANDS = {
  D: '--save-dev',
}

test('getOptionCompletions()', () => {
  expect(getOptionCompletions(TYPES, SHORTHANDS, '--store-dir')).toStrictEqual([])
  expect(getOptionCompletions(TYPES, SHORTHANDS, '--dev')).toBeUndefined()
  expect(getOptionCompletions(TYPES, SHORTHANDS, '--no-dev')).toBeUndefined()
  expect(getOptionCompletions(TYPES, SHORTHANDS, '-D')).toBeUndefined()
  expect(getOptionCompletions(TYPES, SHORTHANDS, '--unknown')).toBeUndefined()
  expect(getOptionCompletions(TYPES, SHORTHANDS, '--color')).toStrictEqual(['red', 'blue'])
  expect(getOptionCompletions(TYPES, SHORTHANDS, '--')).toBeUndefined()
})

test('getLastOption()', () => {
  expect(
    getLastOption({
      last: '',
      lastPartial: 'f',
      line: 'pnpm i --resolution-strategy f ',
      partial: 'pnpm i --resolution-strategy f',
      point: 30,
      prev: 'f',
      words: 4,
    })
  ).toBe('--resolution-strategy')
  expect(
    getLastOption({
      last: '',
      lastPartial: '',
      line: 'pnpm i --resolution-strategy ',
      partial: 'pnpm i --resolution-strategy ',
      point: 28,
      prev: '--resolution-strategy',
      words: 3,
    })
  ).toBe('--resolution-strategy')
})

test('currentTypedWordType()', () => {
  expect(currentTypedWordType({
    last: '',
    lastPartial: '',
    line: 'pnpm i --resolution-strategy ',
    partial: 'pnpm i --resolution-strategy ',
    point: 29,
    prev: '--resolution-strategy',
    words: 3,
  })).toBe(null)
  expect(currentTypedWordType({
    last: '',
    lastPartial: 'f',
    line: 'pnpm i --resolution-strategy f ',
    partial: 'pnpm i --resolution-strategy f',
    point: 30,
    prev: 'f',
    words: 4,
  })).toBe('value')
  expect(currentTypedWordType({
    last: '',
    lastPartial: 'ex',
    line: 'pnpm add ex --save-dev ',
    partial: 'pnpm add ex',
    point: 11,
    prev: '--save-dev',
    words: 4,
  })).toBe('value')
  expect(currentTypedWordType({
    last: '',
    lastPartial: '--res',
    line: 'pnpm i --res foo ',
    partial: 'pnpm i --res',
    point: 12,
    prev: 'foo',
    words: 4,
  })).toBe('option')
})
