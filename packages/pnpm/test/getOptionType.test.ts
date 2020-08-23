import {
  currentTypedWordType,
  getLastOption,
  getOptionCompletions,
} from '../src/getOptionType'
import test = require('tape')

const TYPES = {
  color: ['red', 'blue', Array],
  dev: Boolean,
  'save-dev': Boolean,
  'store-dir': String,
}

const SHORTHANDS = {
  D: '--save-dev',
}

test('getOptionCompletions()', t => {
  t.deepEqual(getOptionCompletions(TYPES, SHORTHANDS, '--store-dir'), [])
  t.deepEqual(getOptionCompletions(TYPES, SHORTHANDS, '--dev'), undefined)
  t.deepEqual(getOptionCompletions(TYPES, SHORTHANDS, '--no-dev'), undefined)
  t.deepEqual(getOptionCompletions(TYPES, SHORTHANDS, '-D'), undefined)
  t.deepEqual(getOptionCompletions(TYPES, SHORTHANDS, '--unknown'), undefined)
  t.deepEqual(getOptionCompletions(TYPES, SHORTHANDS, '--color'), ['red', 'blue'])
  t.deepEqual(getOptionCompletions(TYPES, SHORTHANDS, '--'), undefined)
  t.end()
})

test('getLastOption()', t => {
  t.equal(
    getLastOption({
      last: '',
      lastPartial: 'f',
      line: 'pnpm i --resolution-strategy f ',
      partial: 'pnpm i --resolution-strategy f',
      point: 30,
      prev: 'f',
      words: 4,
    }),
    '--resolution-strategy'
  )
  t.equal(
    getLastOption({
      last: '',
      lastPartial: '',
      line: 'pnpm i --resolution-strategy ',
      partial: 'pnpm i --resolution-strategy ',
      point: 28,
      prev: '--resolution-strategy',
      words: 3,
    }),
    '--resolution-strategy'
  )
  t.end()
})

test('currentTypedWordType()', t => {
  t.equal(currentTypedWordType({
    last: '',
    lastPartial: '',
    line: 'pnpm i --resolution-strategy ',
    partial: 'pnpm i --resolution-strategy ',
    point: 29,
    prev: '--resolution-strategy',
    words: 3,
  }), null, 'pnpm i --resolution-strategy |')
  t.equal(currentTypedWordType({
    last: '',
    lastPartial: 'f',
    line: 'pnpm i --resolution-strategy f ',
    partial: 'pnpm i --resolution-strategy f',
    point: 30,
    prev: 'f',
    words: 4,
  }), 'value', 'pnpm i --resolution-strategy f|')
  t.equal(currentTypedWordType({
    last: '',
    lastPartial: 'ex',
    line: 'pnpm add ex --save-dev ',
    partial: 'pnpm add ex',
    point: 11,
    prev: '--save-dev',
    words: 4,
  }), 'value', 'pnpm add ex| --save-dev')
  t.equal(currentTypedWordType({
    last: '',
    lastPartial: '--res',
    line: 'pnpm i --res foo ',
    partial: 'pnpm i --res',
    point: 12,
    prev: 'foo',
    words: 4,
  }), 'option', 'pnpm i --res| foo')
  t.end()
})
