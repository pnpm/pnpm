import fs from 'fs'
import { node } from '@pnpm/plugin-commands-nvm'
import { tempDir } from '@pnpm/prepare'

test('run specific version of Node.js', async () => {
  tempDir()
  const { exitCode } = await node.handler({
    argv: {
      original: ['node', '-e', 'require("fs").writeFileSync("version",process.version, "utf8")'],
    },
    useNodeVersion: '14.0.0',
    pnpmHomeDir: process.cwd(),
    rawConfig: {},
  })
  expect(exitCode).toBe(0)
  expect(fs.readFileSync('version', 'utf8')).toBe('v14.0.0')
})

test('run LTS version of Node.js by default', async () => {
  tempDir()
  const { exitCode } = await node.handler({
    argv: {
      original: ['node', '-e', 'require("fs").writeFileSync("version",process.version, "utf8")'],
    },
    pnpmHomeDir: process.cwd(),
    rawConfig: {},
  })
  expect(exitCode).toBe(0)
  expect(fs.readFileSync('version', 'utf8')).toMatch(/^v[0-9]+\.[0-9]+\.[0-9]+$/)
})
