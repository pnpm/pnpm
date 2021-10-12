import child from 'child_process'
import path from 'path'
import fs from 'fs'
import exists from 'path-exists'

const fixtures = path.join(__dirname, 'fixture')

test('pnpm-log is created on fail', () => {
  const fixture = path.join(fixtures, '1')
  child.spawnSync('node', [path.join(fixture, 'index.js')], { cwd: fixture })
  const actual = fs.readFileSync(path.join(fixture, 'node_modules/.pnpm-debug.log'), 'utf-8')
  const expected = fs.readFileSync(path.join(fixture, 'stdout'), 'utf-8')
  expect(actual).toBe(expected)
})

test('pnpm-log is not created on fail if the writeDebugLogFile global variable is set to false', async () => {
  const fixture = path.join(fixtures, '3')
  child.spawnSync('node', [path.join(fixture, 'index.js')], { cwd: fixture })
  expect(!await exists(path.join(fixture, 'node_modules/.pnpm-debug.log'))).toBeTruthy()
})

test('pnpm-log is not created on success', async () => {
  const fixture = path.join(fixtures, '2')
  child.spawnSync('node', [path.join(fixture, 'index.js')], { cwd: fixture })
  expect(!await exists(path.join(fixture, 'node_modules/.pnpm-debug.log'))).toBeTruthy() // log file is not created when 0 exit code
})
