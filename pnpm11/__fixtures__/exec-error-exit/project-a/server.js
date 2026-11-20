import { createServer } from 'http'
const server = createServer()
server.listen(process.env.FOO_PORT, () => {
  console.log(`[foo] server listen on ${process.env.FOO_PORT}`)
})
