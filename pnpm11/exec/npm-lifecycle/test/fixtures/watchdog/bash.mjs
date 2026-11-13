import { lifecycle } from '../../../lib/index.js'

const [directory, scriptShell, exportedOptions] = process.argv.slice(2)
const noop = () => {}
await lifecycle({
  name: 'exported-function',
  version: '1.0.0',
  scripts: {
    start: exportedOptions === 'true'
      ? 'greet; if [ "$BASH_VERSINFO" -ge 4 ]; then shopt -q nullglob || exit 21; fi; if (false | true); then exit 22; fi'
      : 'greet',
  },
}, 'start', directory, {
  dir: directory,
  scriptShell,
  stdio: 'pipe',
  unsafePerm: true,
  log: { info: noop, verbose: noop, silly: noop, warn: noop, pause: noop, resume: noop },
})
