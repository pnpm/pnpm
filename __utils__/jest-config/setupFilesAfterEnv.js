import path from 'path'
import { finishWorkers } from '@pnpm/worker'
import { isCI } from 'ci-info'
import { jest } from '@jest/globals';

const pnpmBinDir = path.join(import.meta.dirname, 'node_modules/.bin')
process.env.PATH = `${pnpmBinDir}${path.delimiter}${process.env.PATH}`

afterAll(async () => {
  await finishWorkers()
})

if (isCI) {
  // In CI, retry failed tests up to 2 times to mitigate flakiness, and log errors before retrying for better debugging
  jest.retryTimes(2, { logErrorsBeforeRetry: true })
}
