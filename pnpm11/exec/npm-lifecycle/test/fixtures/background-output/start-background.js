'use strict'

const { spawn } = require('node:child_process')

const background = spawn(process.execPath, ['-e', 'setTimeout(() => console.log("late"), 2000); setTimeout(() => {}, 30000)'], {
  detached: true,
  stdio: ['ignore', 'inherit', 'inherit'],
})
background.unref()
console.log(`background pid ${background.pid}`)
