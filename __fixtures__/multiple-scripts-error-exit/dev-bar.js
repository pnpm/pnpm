import { createServer } from 'http'
const server = createServer()
server.listen(9999, (err) => {
  if (err) {
    console.log(`[bar] dev error:`, err)
  }
  console.log(`[bar] server listen on 9999`)
  setTimeout(() => {
    throw new Error('[bar] server error, Oops')
  }, 2000)
})
