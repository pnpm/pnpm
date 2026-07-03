import http from 'node:http'
import type { AddressInfo } from 'node:net'

import { expect, test } from '@jest/globals'
import { prepare, preparePackages } from '@pnpm/prepare'
import { fixtures } from '@pnpm/test-fixtures'
import getPort from 'get-port'
import isWindows from 'is-windows'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpmSync, spawnPnpm } from './utils/index.js'
import { isPortInUse } from './utils/isPortInUse.js'

const f = fixtures(import.meta.dirname)
const multipleScriptsErrorExit = f.find('multiple-scripts-error-exit')
const testOnPosix = isWindows() ? test.skip : test

test('should print json format error when publish --json failed', async () => {
  prepare({
    name: 'test-publish-package-no-version',
    version: undefined,
  })

  const { status, stdout } = execPnpmSync(['publish', '--dry-run', '--json'])

  expect(status).toBe(1)
  const { error } = JSON.parse(stdout.toString())
  expect(error?.code).toBe('ERR_PNPM_PACKAGE_VERSION_NOT_FOUND')
  expect(error?.message).toBe('Package version is not defined in the package.json.')
})

test('should print webauth URLs in json format error when OTP is required non-interactively', async () => {
  prepare({
    name: 'test-dist-tag-webauth',
    version: '1.0.0',
  })

  const requests: Array<{ method: string | undefined, url: string | undefined }> = []
  const server = http.createServer((req, res) => {
    requests.push({
      method: req.method,
      url: req.url,
    })
    res.writeHead(401, { 'content-type': 'application/json' })
    res.end(JSON.stringify({
      authUrl: 'https://registry.npmjs.org/-/auth/login/abc123',
      doneUrl: 'https://registry.npmjs.org/-/auth/done/abc123',
    }))
  })

  try {
    await new Promise<void>((resolve) => {
      server.listen(0, '127.0.0.1', resolve)
    })
    const { port } = server.address() as AddressInfo
    const proc = spawnPnpm([
      'dist-tag',
      'add',
      'test-dist-tag-webauth@1.0.0',
      'beta',
      '--json',
      `--registry=http://127.0.0.1:${port}/`,
    ])
    const stdout: Buffer[] = []
    const stderr: Buffer[] = []
    proc.stdout!.on('data', (chunk: Buffer) => stdout.push(chunk))
    proc.stderr!.on('data', (chunk: Buffer) => stderr.push(chunk))
    const status = await new Promise<number | null>((resolve, reject) => {
      proc.on('error', reject)
      proc.on('close', (code: number | null, signal: string | null) => {
        if (signal) {
          reject(new Error(`Killed by signal ${signal}\n\n${Buffer.concat([...stdout, ...stderr]).toString()}`))
        } else {
          resolve(code)
        }
      })
    })

    expect(status).toBe(1)
    expect(requests).toEqual([{
      method: 'PUT',
      url: '/-/package/test-dist-tag-webauth/dist-tags/beta',
    }])
    const { error } = JSON.parse(Buffer.concat(stdout).toString())
    expect(error).toMatchObject({
      code: 'ERR_PNPM_OTP_NON_INTERACTIVE',
      authUrl: 'https://registry.npmjs.org/-/auth/login/abc123',
      doneUrl: 'https://registry.npmjs.org/-/auth/done/abc123',
    })
  } finally {
    await new Promise<void>((resolve, reject) => {
      server.close((err) => {
        if (err) reject(err)
        else resolve()
      })
    })
  }
})

test('should print json format error when add dependency on workspace root', async () => {
  preparePackages([
    {
      name: 'project-a',
      version: '1.0.0',
    },
    {
      name: 'project-b',
      version: '1.0.0',
    },
  ])
  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  const { status, stdout } = execPnpmSync(['add', 'nanoid', '--parseable'])

  expect(status).toBe(1)
  const { error } = JSON.parse(stdout.toString())
  expect(error?.code).toBe('ERR_PNPM_ADDING_TO_ROOT')
})

// This test started to fail on Windows for unknown reason.
testOnPosix('should clean up child processes when process exited', async () => {
  const fooPort = await getPort()
  const barPort = await getPort()
  process.chdir(multipleScriptsErrorExit)
  execPnpmSync(['run', '/^dev:.*/'], {
    stdio: 'pipe',
    env: {
      FOO_PORT: fooPort.toString(),
      BAR_PORT: barPort.toString(),
    },
  })
  expect(await isPortInUse(fooPort)).toBe(false)
  expect(await isPortInUse(barPort)).toBe(false)
})

test('should print error summary when some packages fail with --no-bail', async () => {
  preparePackages([
    {
      location: 'project-1',
      package: {
        scripts: {
          build: 'echo "build project-1"',
        },
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',
      scripts: {
        build: 'exit 1',
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',
      scripts: {
        build: 'echo "build project-3"',
      },
    },
  ])
  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  const { stdout } = execPnpmSync(['-r', '--no-bail', 'run', 'build'])
  const output = stdout.toString()
  expect(output).toContain('ERR_PNPM_RECURSIVE_FAIL')
  expect(output).toContain('Summary: 1 fails, 2 passes')
  expect(output).toContain('[ERROR] project-2@1.0.0 build: `exit 1`')
})
