import { expect, test } from '@jest/globals'
import { reportVerifiedFileIntegrity } from '@pnpm/installing.deps-installer'
import { streamParser } from '@pnpm/logger'

// Both message strings are a cross-stack contract: pacquet renders the
// same ones from the same figures.
test('store verification that took long enough is reported with its time', () => {
  const messages = captureInfo(() => {
    reportVerifiedFileIntegrity({ files: 1234, ms: 2450 })
  })

  expect(messages).toEqual(['The integrity of 1234 files was checked in 2.5s.'])
})

// A tie rounds up, in the direction `toFixed` takes it: 2.25s must not
// render as `2.3s` here and `2.2s` in pacquet.
test('a tie in the seconds rounds up', () => {
  const messages = captureInfo(() => {
    reportVerifiedFileIntegrity({ files: 7, ms: 2250 })
  })

  expect(messages).toEqual(['The integrity of 7 files was checked in 2.3s.'])
})

// Under the time threshold there is no time worth naming, so the
// message points at what keeps invalidating the store instead.
test('quick verification of many files is reported as churn', () => {
  const messages = captureInfo(() => {
    reportVerifiedFileIntegrity({ files: 1001, ms: 80 })
  })

  expect(messages).toEqual(['The integrity of 1001 files was checked, because their timestamps changed since the store recorded them. A backup tool, an antivirus scan, or a copied store can cause this.'])
})

test('verification below both thresholds is not reported', () => {
  for (const verified of [
    { files: 1000, ms: 1000 },
    { files: 12, ms: 3 },
    { files: 0, ms: 0 },
  ]) {
    expect(captureInfo(() => {
      reportVerifiedFileIntegrity(verified)
    })).toEqual([])
  }
})

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
