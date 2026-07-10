import http from 'node:http'
import type { AddressInfo } from 'node:net'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { prepare, preparePackages } from '@pnpm/prepare'
import { fixtures } from '@pnpm/test-fixtures'
import getPort from 'get-port'
import isWindows from 'is-windows'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpmSync, spawnPnpm, waitForPnpmExit } from './utils/index.js'
import { isPortInUse } from './utils/isPortInUse.js'

const f = fixtures(import.meta.dirname)
const multipleScriptsErrorExit = f.find('multiple-scripts-error-exit')
const execErrorExit = f.find('exec-error-exit')
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
    name: 'test-publish-webauth',
    version: '1.0.0',
  })

  const requests: Array<{ method: string | undefined, url: string | undefined }> = []
  const server = http.createServer((req, res) => {
    requests.push({
      method: req.method,
      url: req.url,
    })
    res.writeHead(401, {
      'content-type': 'application/json',
      'www-authenticate': 'OTP',
    })
    res.end(JSON.stringify({
      authUrl: 'https://registry.npmjs.org/-/auth/login/abc123',
      doneUrl: 'https://registry.npmjs.org/-/auth/done/abc123',
    }))
  })

  try {
    await new Promise<void>((resolve, reject) => {
      server.once('error', reject)
      server.listen(0, '127.0.0.1', resolve)
    })
    const { port } = server.address() as AddressInfo
    const proc = spawnPnpm([
      'publish',
      '--json',
      '--no-git-checks',
      `--registry=http://127.0.0.1:${port}/`,
    ])
    const { status, stdout } = await waitForPnpmExit(proc)

    expect(status).toBe(1)
    expect(requests).toEqual([{
      method: 'PUT',
      url: '/test-publish-webauth',
    }])
    const { error } = JSON.parse(stdout.toString())
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

test('should clean up the process trees of running commands when a recursive exec fails', async () => {
  const fooPort = await getPort()
  process.chdir(execErrorExit)
  // Remove the wmic and powershell directories from PATH to emulate a
  // Windows installation where enumerating descendant processes is not
  // possible (wmic is removed on modern Windows, and the PowerShell
  // fallback regularly exceeds its 500 ms budget on real machines - the
  // scenario of https://github.com/pnpm/pnpm/issues/12406). The cleanup
  // then has to go through the tracked child PIDs instead. taskkill lives
  // in System32 itself and stays reachable.
  const pathKey = Object.keys(process.env).find((key) => key.toLowerCase() === 'path') ?? 'PATH'
  const pathWithoutProcessEnumerators = (process.env[pathKey] ?? '')
    .split(path.delimiter)
    .filter((dir) => !/wbem|windowspowershell/i.test(dir))
    .join(path.delimiter)
  const { status, stdout } = execPnpmSync(['--recursive', 'exec', 'node', 'script.js'], {
    stdio: 'pipe',
    env: {
      FOO_PORT: fooPort.toString(),
      ...(isWindows() ? { [pathKey]: pathWithoutProcessEnumerators } : {}),
    },
  })
  expect(status).toBe(1)
  expect(stdout.toString()).toContain('ERR_PNPM_RECURSIVE_EXEC_FIRST_FAIL')
  expect(await isPortInUse(fooPort)).toBe(false)
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
