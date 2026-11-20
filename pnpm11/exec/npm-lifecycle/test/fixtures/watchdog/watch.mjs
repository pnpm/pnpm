// Watches the process groups led by the pids on the command line, releases
// the first, and prints `watching`. It then stays alive until it is killed,
// or with `--exit` first, returns at once.
import { watchProcessGroup } from '../../../lib/index.js'

const args = process.argv.slice(2)
const exit = args[0] === '--exit'
const [released, ...watched] = (exit ? args.slice(1) : args).map(Number)
watchProcessGroup(released).release()
for (const leader of watched) {
  watchProcessGroup(leader)
}
process.stdout.write('watching\n')
if (!exit) setInterval(() => {}, 1000)
