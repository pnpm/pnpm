import { type LogBase, logger, streamParser } from '@pnpm/logger'

test('logger logs', async () => {
  const promise = new Promise<LogBase>(resolve => {
    streamParser.on('data', function handle (msg) {
      streamParser.removeListener('data', handle)
      resolve(msg)
    })
  })
  logger.info({ message: 'foo', prefix: 'bar' })
  expect(await promise).toMatchObject({
    name: 'pnpm',
    level: 'info',
    message: 'foo',
    prefix: 'bar',
  })
})
