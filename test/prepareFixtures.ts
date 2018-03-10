import pnpm from '@pnpm/exec'
import path = require('path')

run()
  .then(() => console.log('Success!'))
  .catch(err => console.error(err))

async function run () {
  await pnpm(['install', '--shrinkwrap-only'], {cwd: path.join(__dirname, 'fixtures/simple')})
}
