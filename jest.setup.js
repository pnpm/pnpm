jest.retryTimes(1);

afterAll(() => {
  // @ts-expect-error
  global.finishWorkers?.()
})

// Polyfilling Symbol.asyncDispose for Jest.
//
// Copied with a few changes from https://devblogs.microsoft.com/typescript/announcing-typescript-5-2/#using-declarations-and-explicit-resource-management
if (Symbol.asyncDispose === undefined) {
  Symbol.asyncDispose = Symbol('Symbol.asyncDispose')
}
