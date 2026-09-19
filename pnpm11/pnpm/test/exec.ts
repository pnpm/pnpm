import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'
import { writeYamlFileSync } from 'write-yaml-file'

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

test('exec and the run fallback find the project bins from a subdirectory', async () => {
  prepare({
    name: 'root',
    version: '1.0.0',
    dependencies: {
      '@pnpm.e2e/hello-world-js-bin': '1.0.0',
    },
  })
  await execPnpm(['install'])
  const subdirPath = path.join(process.cwd(), 'some-directory')
  fs.mkdirSync(subdirPath)

  for (const args of [['exec', 'hello-world-js-bin'], ['hello-world-js-bin']]) {
    const result = execPnpmSync(args, { cwd: subdirPath, expectSuccess: true })
    expect(result.stdout.toString()).toContain('Hello world!')
  }
})

test('silent exec does not print verifyDepsBeforeRun install output', async () => {
  prepare({})
  writeYamlFileSync('pnpm-workspace.yaml', {
    verifyDepsBeforeRun: 'install',
  })

  const result = execPnpmSync(['--silent', 'exec', 'node', '-e', 'process.stdout.write("hi")'], {
    expectSuccess: true,
    omitEnvDefaults: ['pnpm_config_silent'],
  })

  expect(result.stdout.toString()).toBe('hi')
})
