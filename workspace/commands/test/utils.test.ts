import { expect, test } from '@jest/globals'

import { personToString } from '../lib/utils.js'

test('run the personToString function', () => {
  const expectAuthor = 'pnpm <xxxxxx@pnpm.com> (https://www.github.com/pnpm)'
  expect(personToString({
    email: 'xxxxxx@pnpm.com',
    name: 'pnpm',
    url: 'https://www.github.com/pnpm',
  })).toBe(expectAuthor)
})
