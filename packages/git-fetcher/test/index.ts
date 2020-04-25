///<reference path="../../../typings/index.d.ts"/>
import createCafs from '@pnpm/cafs'
import createFetcher from '@pnpm/git-fetcher'
import test = require('tape')
import tempy = require('tempy')

test('fetch', async t => {
  const cafsDir = tempy.directory()
  t.comment(`cafs at ${cafsDir}`)
  const fetch = createFetcher().git
  const fetchResult = await fetch({
    commit: 'c9b30e71d704cd30fa71f2edd1ecc7dcc4985493',
    repo: 'https://github.com/kevva/is-positive.git',
  }, {
    cafs: createCafs(cafsDir),
  })
  t.ok(fetchResult.filesIndex['package.json'])
  t.ok(await fetchResult.filesIndex['package.json'].generatingIntegrity)
  t.end()
})
