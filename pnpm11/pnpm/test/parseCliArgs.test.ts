import { expect, test } from '@jest/globals'

import { getCliOptionsTypes, getCommandFullName, pnpmCmds } from '../src/cmd/index.js'
import { parseCliArgs } from '../src/parseCliArgs.js'

test('the "issues" alias resolves to the "bugs" command', async () => {
  expect(getCommandFullName('issues')).toBe('bugs')
  expect(pnpmCmds.issues).toBe(pnpmCmds.bugs)
  expect(getCliOptionsTypes('issues')).toHaveProperty(['registry'])

  const { cmd } = await parseCliArgs(['issues', 'is-positive'])
  expect(cmd).toBe('bugs')
})
