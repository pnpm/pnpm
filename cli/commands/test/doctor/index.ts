import { doctor } from '@pnpm/cli.commands'

test('doctor handler runs without error', async () => {
  await doctor.handler()
})
