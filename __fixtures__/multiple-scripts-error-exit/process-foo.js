import { createServer } from 'http'
const server = createServer()
server.listen(process.env.FOO_PORT, (err) => {
  if (err) {
    console.log(`[foo] dev error:`, err)
  }
  console.log(`[foo] server listen on ${process.env.FOO_PORT}`)
})
