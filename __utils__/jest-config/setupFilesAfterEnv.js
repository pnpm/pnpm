import path from 'path'
import { finishWorkers } from '@pnpm/worker'

const pnpmBinDir = path.join(import.meta.dirname, 'node_modules/.bin')
process.env.PATH = `${pnpmBinDir}${path.delimiter}${process.env.PATH}`

afterAll(async () => {
  await finishWorkers()
})
