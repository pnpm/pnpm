const { spawn } = require('node:child_process')
const path = require('node:path')

const child = spawn(process.execPath, [path.join(__dirname, 'child.cjs')], { stdio: 'ignore' })
child.unref()
console.log(child.pid)
setInterval(() => {}, 1000)
