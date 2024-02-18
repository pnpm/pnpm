const { start, prepare } = require('@pnpm/registry-mock')

module.exports = () => {
  prepare()
  const server = start({
    stdio: 'ignore',
  })
  let killed = false
  server.on('close', () => {
    if (!killed) {
      console.log('Error: The registry server was killed!')
      process.exit(1)
    }
  })
  global.killServer = () => {
    killed = true
    server.kill()
  }
}
