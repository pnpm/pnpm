import fs from 'fs'
import path from 'path'
import { prepare } from '@pnpm/prepare'
import { execPnpm, execPnpmSync } from './utils/index.js'

test("exec should respect the caller's current working directory", async () => {
  prepare({
    name: 'root',
    version: '1.0.0',
  })

  const projectRoot = process.cwd()
  fs.mkdirSync('some-directory', { recursive: true })
  const subdirPath = path.join(projectRoot, 'some-directory')

  await execPnpm(['install'])

  const cmdFilePath = path.join(subdirPath, 'cwd.txt')

  execPnpmSync(
    ['exec', 'node', '-e', `require('fs').writeFileSync(${JSON.stringify(cmdFilePath)}, process.cwd(), 'utf8')`],
    {
      cwd: subdirPath,
      expectSuccess: true,
    }
  )

  expect(fs.readFileSync(cmdFilePath, 'utf8')).toBe(subdirPath)
})
