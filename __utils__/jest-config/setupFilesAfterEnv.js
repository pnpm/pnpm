import path from 'path'
import { finishWorkers } from '@pnpm/worker'
import { isCI } from 'ci-info'

const pnpmBinDir = path.join(import.meta.dirname, 'node_modules/.bin')
process.env.PATH = `${pnpmBinDir}${path.delimiter}${process.env.PATH}`

afterAll(async () => {
  await finishWorkers()
})

if (isCI) {
  jest.retryTimes(3, { logErrorsBeforeRetry: true })
}
