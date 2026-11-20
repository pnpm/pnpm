import { lifecycle } from '../../../lib/index.js'

const [directory, scriptShell, mode] = process.argv.slice(2)
const scripts = {
  false: 'greet',
  true: 'greet; if [ "$BASH_VERSINFO" -ge 4 ]; then shopt -q nullglob || exit 21; fi; if (false | true); then exit 22; fi',
  'non-identifier': 'env | grep -qx "NON-IDENTIFIER=kept" || exit 23',
}
const noop = () => {}
await lifecycle({
  name: 'exported-function',
  version: '1.0.0',
  scripts: { start: scripts[mode] },
}, 'start', directory, {
  dir: directory,
  scriptShell,
  stdio: 'pipe',
  unsafePerm: true,
  log: { info: noop, verbose: noop, silly: noop, warn: noop, pause: noop, resume: noop },
})
