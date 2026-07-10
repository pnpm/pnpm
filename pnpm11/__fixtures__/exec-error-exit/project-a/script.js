import { spawn } from 'child_process'
const child = spawn(process.execPath, ['./server.js'], { stdio: 'ignore' })
child.unref()
setInterval(() => {}, 1000)
