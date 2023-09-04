jest.retryTimes(1, {
  // Some tests don't clean up their resources completely and cause the retried
  // run fail. Set logErrorsBeforeRetry to make it more clear that the retried
  // run was a retry and not the first attempt. Otherwise Jest hides the first
  // attempt. This makes it easier to distinguish between a test that's truly
  // broken and one that's not retry-able.
  logErrorsBeforeRetry: true
})

afterAll(() => {
  // @ts-expect-error
  global.finishWorkers?.()
})
