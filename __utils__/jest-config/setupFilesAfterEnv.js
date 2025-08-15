import { finishWorkers } from '@pnpm/worker'

afterAll(async () => {
  await finishWorkers()
})
