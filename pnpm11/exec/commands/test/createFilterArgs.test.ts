import { describe, expect, test } from '@jest/globals'

import { createFilterArgs } from '../src/runDepsStatusCheck.js'

describe('createFilterArgs', () => {
  test.each([
    [{}, []],
    [{ filter: [] }, []],
    [{ filter: ['foo'] }, ['--filter=foo']],
    [{ filter: ['foo', 'bar'] }, ['--filter=foo', '--filter=bar']],
    [{ filterProd: ['foo'] }, ['--filter-prod=foo']],
    [{ filter: ['foo'], filterProd: ['bar'] }, ['--filter=foo', '--filter-prod=bar']],
  ])('%o -> %o', (opts, expected) => {
    expect(createFilterArgs(opts)).toStrictEqual(expected)
  })
})
