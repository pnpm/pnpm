const path = require('path')
const { finishWorkers } = require('@pnpm/worker')

// Code under test that runs `pnpm` (preparing a git-hosted dependency, for
// example) should get the pnpm built from this repository, not whichever pnpm
// runs the tests.
const pnpmBinDir = path.join(__dirname, 'node_modules/.bin')
process.env.PATH = `${pnpmBinDir}${path.delimiter}${process.env.PATH}`

afterAll(async () => {
  await finishWorkers()
})
