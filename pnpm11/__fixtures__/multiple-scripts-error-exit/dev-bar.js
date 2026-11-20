import { createServer } from 'http'
const server = createServer()
server.listen(process.env.BAR_PORT, (err) => {
  if (err) {
    console.log(`[bar] dev error:`, err)
  }
  console.log(`[bar] server listen on ${process.env.BAR_PORT}`)
  setTimeout(() => {
    throw new Error('[bar] server error, Oops')
  }, 2000)
})
