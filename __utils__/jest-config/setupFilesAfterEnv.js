const { finishWorkers } = require('@pnpm/worker')

afterAll(async () => {
  await finishWorkers()
})
