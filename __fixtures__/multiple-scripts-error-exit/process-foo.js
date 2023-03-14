import { createServer } from 'http'
const server = createServer()
server.listen(9990, (err) => {
  if (err) {
    console.log(`[foo] dev error:`, err)
  }
  console.log(`[foo] server listen on 9990`)
})
