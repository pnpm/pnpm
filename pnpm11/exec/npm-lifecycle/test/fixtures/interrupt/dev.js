// Shuts down on the first SIGINT or SIGTERM and exits at once on a second
// interrupt, the way many CLIs treat a repeated Ctrl+C. Marker files record
// what happened, since the runner's exit is what the tests observe; the
// started marker holds the pid, so a test can tell whether the script is
// still running once the runner is gone.
const fs = require('node:fs')
let interrupts = 0
const shutDown = () => {
  setTimeout(() => {
    fs.writeFileSync('shut-down.txt', '')
    console.log('shut down')
    process.exit(0)
  }, 500)
}
process.on('SIGTERM', shutDown)
process.on('SIGINT', () => {
  interrupts += 1
  if (interrupts > 1) {
    fs.writeFileSync('forced.txt', '')
    console.log('forced')
    process.exit(130)
  }
  shutDown()
})
fs.writeFileSync('started.txt', String(process.pid))
console.log('started')
setInterval(() => {}, 1000)
