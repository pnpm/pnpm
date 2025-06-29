import { spawn } from 'node:child_process'
spawn('node', ['./process-foo.js'], { stdio: 'inherit' })
