import { execFileSync } from 'node:child_process'
import { readFileSync } from 'node:fs'

const paths = execFileSync('git', ['ls-files', '-z', '--', '*.rs', '*.ts'], { encoding: 'utf8' })
  .split('\0')
  .filter(Boolean)
const separator = /-{6,}/
const tableDivider = /^\s*\/\/[!/]?\s*\|[\s:|=-]+\|\s*$/
let failed = false

for (const path of paths) {
  const lines = readFileSync(path, 'utf8').split(/\r?\n/)
  for (let index = 0; index < lines.length; index++) {
    const line = lines[index]
    if (!/^\s*\/\//.test(line) || !separator.test(line) || tableDivider.test(line)) continue
    console.error(`${path}:${index + 1}: remove decorative comment separators`)
    failed = true
  }
}

if (failed) process.exitCode = 1
