/// <reference path="../../../typings/index.d.ts"/>
import path from 'path'
import createFetcher from '@pnpm/directory-fetcher'

test('fetch', async () => {
  const fetcher = createFetcher()

  // eslint-disable-next-line
  const fetchResult = await fetcher.directory({} as any, {
    directory: '..',
    type: 'directory',
  }, {
    lockfileDir: __dirname,
  })

  expect(fetchResult.local).toBe(true)
  expect(fetchResult.packageImportMethod).toBe('hardlink')
  expect(fetchResult.filesIndex['package.json']).toBe(path.join(__dirname, '../package.json'))

  // Only those files are included which would get published
  expect(Object.keys(fetchResult.filesIndex).sort()).toStrictEqual([
    'README.md',
    'lib/index.d.ts',
    'lib/index.js',
    'lib/index.js.map',
    'package.json',
  ])
})
