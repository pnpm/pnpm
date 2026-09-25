import { spawn } from 'child_process'
spawn('node', ['./process-foo.js'], { stdio: 'inherit' })
