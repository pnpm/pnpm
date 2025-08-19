import net from 'node:net'
import { computeHandlePath } from './computeHandlePath.js'

const [handle] = process.argv.slice(2)
const connectPath = computeHandlePath(handle)

const client = net.connect(connectPath, () => {
  process.stdin.pipe(client).on('end', () => client.destroy())
})
