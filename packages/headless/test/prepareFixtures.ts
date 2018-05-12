import pnpm from '@pnpm/exec'
import path = require('path')

const fixtures = path.join(__dirname, 'fixtures')

run()
  .then(() => console.log('Success!'))
  .catch(err => console.error(err))

async function run () {
  // TODO: remove --ignore-scripts. pnpm should ignore scripts when --shrinkwrap-only is used
  await pnpm(['recursive', 'install', '--shrinkwrap-only', '--registry', 'http://localhost:4873/', '--ignore-scripts', '--force'], {cwd: fixtures})
}
