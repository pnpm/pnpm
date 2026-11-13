import { lifecycle } from '../../../lib/index.js'

const [directory, scriptShell] = process.argv.slice(2)
const noop = () => {}
await lifecycle({
  name: 'exported-function',
  version: '1.0.0',
  scripts: { start: 'greet' },
}, 'start', directory, {
  dir: directory,
  scriptShell,
  stdio: 'pipe',
  unsafePerm: true,
  log: { info: noop, verbose: noop, silly: noop, warn: noop, pause: noop, resume: noop },
})
