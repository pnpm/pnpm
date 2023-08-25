jest.retryTimes(1);

afterAll(() => {
  // @ts-expect-error
  global.finishWorkers?.()
})
