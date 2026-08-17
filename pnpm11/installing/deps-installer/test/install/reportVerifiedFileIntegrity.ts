import { expect, test } from '@jest/globals'
import { reportVerifiedFileIntegrity } from '@pnpm/installing.deps-installer'
import { streamParser } from '@pnpm/logger'

function captureInfo (run: () => void): string[] {
  const messages: string[] = []
  const reporter = (log: { level?: string, message?: string }) => {
    if (log.level === 'info' && log.message != null) messages.push(log.message)
  }
  streamParser.on('data', reporter as never)
  try {
    run()
  } finally {
    streamParser.removeListener('data', reporter as never)
  }
  return messages
}

// The message string is a cross-stack contract: pacquet renders the
// same one from the same figures.
test('store verification that took long enough is reported with its time and file count', () => {
  const messages = captureInfo(() => {
    reportVerifiedFileIntegrity({ files: 1234, ms: 2450 })
  })

  expect(messages).toContain('The integrity of 1234 files was checked in 2.5s. This might have caused installation to take longer.')
})

test('store verification under the threshold stays quiet, however many files it covered', () => {
  const messages = captureInfo(() => {
    reportVerifiedFileIntegrity({ files: 100_000, ms: 1000 })
  })

  expect(messages).toEqual([])
})
