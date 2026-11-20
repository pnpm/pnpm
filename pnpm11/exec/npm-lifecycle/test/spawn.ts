import { expect, jest, test } from '@jest/globals'

import { spawn } from '../src/spawn.js'

test('progress is enabled again after a child that could not be spawned', async () => {
  const log = {
    progressEnabled: true,
    disableProgress: jest.fn(),
    enableProgress: jest.fn(),
  }

  const missing = spawn('this-command-does-not-exist', [], { stdio: 'inherit', log })
  await new Promise<void>((resolve) => {
    missing.once('error', () => {
      resolve()
    })
  })
  await new Promise<void>((resolve) => {
    setImmediate(resolve)
  })

  const existing = spawn(process.execPath, ['-e', ''], { stdio: 'inherit', log })
  await new Promise<void>((resolve) => {
    existing.once('close', () => {
      resolve()
    })
  })

  expect(log.disableProgress).toHaveBeenCalled()
  expect(log.enableProgress).toHaveBeenCalledTimes(2)
})
