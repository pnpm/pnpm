// Runs the fixture's script named on the command line (`dev` by default)
// through lifecycle() with inherited stdio, as `pnpm run` does. `dev` execs
// node so that the shell is out of the picture; `dev-behind-shell` keeps the
// shell as node's parent, which is what a `sh` such as dash 0.5.12 does.
import fs from 'node:fs'
import path from 'node:path'

import { lifecycle } from '../../../lib/index.js'

const fixture = import.meta.dirname
const pkg = JSON.parse(fs.readFileSync(path.join(fixture, 'package.json'), 'utf8'))
const noop = () => {}
const log = { info: noop, warn: noop, silly: noop, verbose: noop, pause: noop, resume: noop }
try {
  await lifecycle(pkg, process.argv[2] ?? 'dev', fixture, { stdio: 'inherit', log, dir: fixture })
} catch (err) {
  console.log(`lifecycle failed: ${err.message}`)
  process.exit(1)
}
