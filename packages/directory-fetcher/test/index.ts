/// <reference path="../../../typings/index.d.ts"/>
import createFetcher from '@pnpm/directory-fetcher'

test('fetch', async () => {
  const fetcher = createFetcher()

  expect(typeof fetcher.directory).toBe('function')
})
