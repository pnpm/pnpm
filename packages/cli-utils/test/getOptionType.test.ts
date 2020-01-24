import { getLastOption, getOptionCompletions } from '@pnpm/cli-utils'
import test = require('tape')

const TYPES = {
  'color': ['red', 'blue', Array],
  'dev': Boolean,
  'save-dev': Boolean,
  'store-dir': String,
}

const SHORTHANDS = {
  'D': '--save-dev',
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
    '--resolution-strategy',
  )
  t.equal(
    getLastOption({
      last: '',
      lastPartial: 'f',
      line: 'pnpm i --resolution-strategy ',
      partial: 'pnpm i --resolution-strategy ',
      point: 28,
      prev: '--resolution-strategy',
      words: 3,
    }),
    '--resolution-strategy',
  )
  t.end()
})
