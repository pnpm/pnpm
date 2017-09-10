import test = require('tape')
import path = require('path')
import outdated from '../src'

process.chdir(path.join(__dirname, 'fixtures'))
const temp = path.join(__dirname, '..', '.tmp')

const outdatedOpts = {
  offline: false,
  storePath: temp,
  strictSsl: true,
  fetchRetries: 2,
  fetchRetryFactor: 10,
  fetchRetryMintimeout: 1e4,
  fetchRetryMaxtimeout: 6e4,
  userAgent: 'pnpm',
  tag: 'latest',
  networkConcurrency: 16,
  rawNpmConfig: {},
  alwaysAuth: false,
}

test('fail when there is no shrinkwrap.yaml file in the root of the project', async t => {
  try {
    await outdated('no-shrinkwrap', outdatedOpts)
    t.fail('the call should have failed')
  } catch (err) {
    t.equal(err.message, 'No shrinkwrapfile in this directory. Run `pnpm install` to generate one.')
    t.end()
  }
})

test('outdated()', async t => {
  const outdatedPkgs = await outdated('wanted-shrinkwrap', outdatedOpts)
  t.deepEqual(outdatedPkgs, [
    {
      packageName: 'is-negative',
      current: '1.0.0',
      wanted: '1.1.0',
      latest: '2.1.0'
    },
    {
      packageName: 'is-positive',
      current: '1.0.0',
      wanted: '3.1.0',
      latest: '3.1.0'
    }
  ])
  t.end()
})
