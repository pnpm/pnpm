import { connect } from 'net'

// Fail only once project-a's grandchild server is listening, so that the
// failure reliably happens while project-a's process tree is still running.
function tryConnect () {
  const socket = connect(Number(process.env.FOO_PORT), '127.0.0.1')
  socket.once('connect', () => {
    socket.destroy()
    process.exit(1)
  })
  socket.once('error', () => {
    socket.destroy()
    setTimeout(tryConnect, 100)
  })
}
tryConnect()
