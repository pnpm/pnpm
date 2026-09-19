// Shuts down on the first SIGINT or SIGTERM and exits at once on a second
// interrupt, the way many CLIs treat a repeated Ctrl+C.
let interrupts = 0
process.on('SIGTERM', () => {
  setTimeout(() => {
    console.log('shut down')
    process.exit(0)
  }, 500)
})
process.on('SIGINT', () => {
  interrupts += 1
  if (interrupts > 1) {
    console.log('forced')
    process.exit(130)
  }
  setTimeout(() => {
    console.log('shut down')
    process.exit(0)
  }, 500)
})
console.log('started')
setInterval(() => {}, 1000)
