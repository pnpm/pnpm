import { readdirSync, readFileSync } from 'node:fs'
import { spawn } from 'node:child_process'
import { once } from 'node:events'
for (const path of ['/proc', '/proc/self/stat', '/proc/self/status']) {
  try { console.log(path, path === '/proc' ? readdirSync(path) : readFileSync(path, 'utf8')) } catch (error) { console.log(path, error.code) }
}
for (const args of [['-eo', 'pid,ppid,args'], []]) {
  const child = spawn('/bin/ps', args)
  child.stdout.on('data', bytes => console.log('ps stdout', bytes.toString()))
  child.stderr.on('data', bytes => console.log('ps stderr', bytes.toString()))
  child.on('error', error => console.log('ps error', error.message))
  await once(child, 'close')
}
