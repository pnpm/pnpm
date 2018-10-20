import outdated, {forPackages as outdatedForPackages} from '@pnpm/outdated'
import path = require('path')
import test = require('tape')

process.chdir(path.join(__dirname, 'fixtures'))
const temp = path.join(__dirname, '..', '.tmp')

const outdatedOpts = {
  alwaysAuth: false,
  fetchRetries: 2,
  fetchRetryFactor: 10,
  fetchRetryMaxtimeout: 6e4,
  fetchRetryMintimeout: 1e4,
  networkConcurrency: 16,
  offline: false,
  rawNpmConfig: {
    registry: 'https://registry.npmjs.org/',
  },
  store: temp,
  strictSsl: true,
  tag: 'latest',
  userAgent: 'pnpm',
}

test('fail when there is no shrinkwrap.yaml file in the root of the project', async (t) => {
  try {
    await outdated('no-shrinkwrap', outdatedOpts)
    t.fail('the call should have failed')
  } catch (err) {
    t.equal(err.message, 'No shrinkwrapfile in this directory. Run `pnpm install` to generate one.')
    t.end()
  }
})

test('dont fail when there is no shrinkwrap.yaml file but no dependencies in package.json', async (t) => {
  t.deepEqual(await outdated('no-deps', outdatedOpts), [])
  t.end()
})

test('outdated()', async (t) => {
  const outdatedPkgs = await outdated('wanted-shrinkwrap', outdatedOpts)
  t.deepEqual(outdatedPkgs, [
    {
      current: 'github.com/blabla/from-github/d5f8d5500f7faf593d32e134c1b0043ff69151b4',
      latest: undefined,
      packageName: 'from-github',
      wanted: 'github.com/blabla/from-github/d5f8d5500f7faf593d32e134c1b0043ff69151b3',
    },
    {
      current: undefined,
      latest: undefined,
      packageName: 'from-github-2',
      wanted: 'github.com/blabla/from-github-2/d5f8d5500f7faf593d32e134c1b0043ff69151b3',
    },
    {
      current: '1.0.0',
      latest: '2.1.0',
      packageName: 'is-negative',
      wanted: '1.1.0',
    },
    {
      current: '1.0.0',
      latest: '3.1.0',
      packageName: 'is-positive',
      wanted: '3.1.0',
    },
  ])
  t.end()
})

test('forPackages()', async (t) => {
  const outdatedPkgs = await outdatedForPackages(['is-negative'], 'wanted-shrinkwrap', outdatedOpts)
  t.deepEqual(outdatedPkgs, [
    {
      current: '1.0.0',
      latest: '2.1.0',
      packageName: 'is-negative',
      wanted: '1.1.0',
    },
  ])
  t.end()
})

test('outdated() when only current shrinkwrap is present', async (t) => {
  const outdatedPkgs = await outdated('current-shrinkwrap-only', outdatedOpts)
  t.deepEqual(outdatedPkgs, [
    {
      current: '1.1.0',
      latest: '2.1.0',
      packageName: 'is-negative',
      wanted: '1.1.0',
    },
  ])
  t.end()
})

test('outdated() on package with external shrinkwrap', async (t) => {
  const outdatedPkgs = await outdated('../external-wanted-shrinkwrap/pkg', {
    ...outdatedOpts,
    shrinkwrapDirectory: path.resolve('../external-wanted-shrinkwrap'),
  })
  t.deepEqual(outdatedPkgs, [
    {
      current: 'github.com/blabla/from-github/d5f8d5500f7faf593d32e134c1b0043ff69151b4',
      latest: undefined,
      packageName: 'from-github',
      wanted: 'github.com/blabla/from-github/d5f8d5500f7faf593d32e134c1b0043ff69151b3',
    },
    {
      current: undefined,
      latest: undefined,
      packageName: 'from-github-2',
      wanted: 'github.com/blabla/from-github-2/d5f8d5500f7faf593d32e134c1b0043ff69151b3',
    },
    {
      current: '1.0.0',
      latest: '2.1.0',
      packageName: 'is-negative',
      wanted: '1.1.0',
    },
    {
      current: '1.0.0',
      latest: '3.1.0',
      packageName: 'is-positive',
      wanted: '3.1.0',
    },
  ])
  t.end()
})
