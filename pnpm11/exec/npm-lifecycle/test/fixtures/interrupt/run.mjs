// Runs the fixture's dev script through lifecycle() with inherited stdio, as
// `pnpm run` does. The script execs node so that the shell is out of the
// picture: a `sh` that keeps running as node's parent (dash 0.5.12 does) takes
// the signals itself, and what it then does is the shell's behavior, not the
// relay's.
import fs from 'node:fs'
import path from 'node:path'

import { lifecycle } from '../../../lib/index.js'

const fixture = import.meta.dirname
const pkg = JSON.parse(fs.readFileSync(path.join(fixture, 'package.json'), 'utf8'))
const noop = () => {}
const log = { info: noop, warn: noop, silly: noop, verbose: noop, pause: noop, resume: noop }
try {
  await lifecycle(pkg, 'dev', fixture, { stdio: 'inherit', log, dir: fixture })
} catch (err) {
  console.log(`lifecycle failed: ${err.message}`)
  process.exit(1)
}
