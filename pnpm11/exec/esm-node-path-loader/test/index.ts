import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { describe, expect, test } from '@jest/globals'
import {
  addEsmNodePathLoaderOption,
  esmNodePathLoaderImportFlag,
  keepEsmNodePathLoaderOption,
} from '@pnpm/exec.esm-node-path-loader'

test('the flag is a self-contained data: URL import', () => {
  expect(esmNodePathLoaderImportFlag).toMatch(/^--import=data:text\/javascript,/)
  // The flag must never contain characters that Node's NODE_OPTIONS
  // tokenizer would split or unquote.
  expect(esmNodePathLoaderImportFlag).not.toMatch(/[\s"'\\]/)
})

test('the flag matches the golden copy shared with the Rust CLI', () => {
  // The Rust CLI derives the same flag from its own embedded copy of the
  // hook sources and asserts against the same file
  // (pnpm/crates/config/src/esm_node_path_loader/tests.rs), so the two
  // stacks cannot drift apart without one of the tests failing.
  const golden = fs.readFileSync(new URL('./import-flag.txt', import.meta.url), 'utf8')
  expect(esmNodePathLoaderImportFlag).toBe(golden)
})

describe('addEsmNodePathLoaderOption', () => {
  test('returns just the flag when NODE_OPTIONS is empty', () => {
    expect(addEsmNodePathLoaderOption(undefined)).toBe(esmNodePathLoaderImportFlag)
    expect(addEsmNodePathLoaderOption('')).toBe(esmNodePathLoaderImportFlag)
  })

  test('appends the flag to existing NODE_OPTIONS', () => {
    expect(addEsmNodePathLoaderOption('--max-old-space-size=4096'))
      .toBe(`--max-old-space-size=4096 ${esmNodePathLoaderImportFlag}`)
  })

  test('does not duplicate the flag', () => {
    const once = addEsmNodePathLoaderOption('--enable-source-maps')
    expect(addEsmNodePathLoaderOption(once)).toBe(once)
  })
})

describe('keepEsmNodePathLoaderOption', () => {
  test('reapplies the flag when the replaced NODE_OPTIONS carried it', () => {
    expect(keepEsmNodePathLoaderOption('--no-warnings', addEsmNodePathLoaderOption(undefined)))
      .toBe(`--no-warnings ${esmNodePathLoaderImportFlag}`)
  })

  test('leaves NODE_OPTIONS alone when the previous value did not carry the flag', () => {
    expect(keepEsmNodePathLoaderOption('--no-warnings', undefined)).toBe('--no-warnings')
    expect(keepEsmNodePathLoaderOption('--no-warnings', '--enable-source-maps')).toBe('--no-warnings')
  })
})

describe('the registered loader', () => {
  test('resolves a phantom ESM import through NODE_PATH', () => {
    const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-esm-node-path-loader-'))
    const phantomDir = path.join(tmp, 'store', 'node_modules', 'phantom-dep')
    fs.mkdirSync(phantomDir, { recursive: true })
    fs.writeFileSync(path.join(phantomDir, 'package.json'), JSON.stringify({ name: 'phantom-dep', version: '1.0.0', main: 'index.js' }))
    fs.writeFileSync(path.join(phantomDir, 'index.js'), 'module.exports = "phantom-resolved"')
    const appDir = path.join(tmp, 'app')
    fs.mkdirSync(appDir)
    const script = path.join(appDir, 'main.mjs')
    fs.writeFileSync(script, 'import dep from "phantom-dep"\nconsole.log(dep)')

    const env = {
      ...process.env,
      NODE_PATH: path.join(tmp, 'store', 'node_modules'),
    }
    const withoutLoader = spawnSync(process.execPath, [script], { env: { ...env, NODE_OPTIONS: '' } })
    expect(withoutLoader.status).not.toBe(0)

    const withLoader = spawnSync(process.execPath, [script], { env: { ...env, NODE_OPTIONS: esmNodePathLoaderImportFlag } })
    expect(withLoader.stderr.toString()).toBe('')
    expect(withLoader.status).toBe(0)
    expect(withLoader.stdout.toString().trim()).toBe('phantom-resolved')
  })

  test('the fallback preserves import conditions for dual-condition packages', () => {
    const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-esm-node-path-loader-'))
    const dualDir = path.join(tmp, 'store', 'node_modules', 'dual-dep')
    fs.mkdirSync(dualDir, { recursive: true })
    fs.writeFileSync(path.join(dualDir, 'package.json'), JSON.stringify({
      name: 'dual-dep',
      version: '1.0.0',
      exports: {
        '.': {
          import: './esm.mjs',
          require: './cjs.js',
        },
      },
    }))
    fs.writeFileSync(path.join(dualDir, 'esm.mjs'), 'export default "esm-target"')
    fs.writeFileSync(path.join(dualDir, 'cjs.js'), 'module.exports = "cjs-target"')
    const script = path.join(tmp, 'main.mjs')
    fs.writeFileSync(script, 'import dep from "dual-dep"\nconsole.log(dep)')

    const result = spawnSync(process.execPath, [script], {
      env: {
        ...process.env,
        NODE_PATH: path.join(tmp, 'store', 'node_modules'),
        NODE_OPTIONS: esmNodePathLoaderImportFlag,
      },
    })
    expect(result.stderr.toString()).toBe('')
    expect(result.status).toBe(0)
    expect(result.stdout.toString().trim()).toBe('esm-target')
  })

  test('a resolution error from a NODE_PATH entry surfaces instead of the not-found error', () => {
    const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-esm-node-path-loader-'))
    const depDir = path.join(tmp, 'store', 'node_modules', 'exported-dep')
    fs.mkdirSync(depDir, { recursive: true })
    fs.writeFileSync(path.join(depDir, 'package.json'), JSON.stringify({
      name: 'exported-dep',
      version: '1.0.0',
      exports: { '.': './index.js' },
    }))
    fs.writeFileSync(path.join(depDir, 'index.js'), 'module.exports = "main"')
    const script = path.join(tmp, 'main.mjs')
    fs.writeFileSync(script, 'await import("exported-dep/unexported")')

    const result = spawnSync(process.execPath, [script], {
      env: {
        ...process.env,
        NODE_PATH: path.join(tmp, 'store', 'node_modules'),
        NODE_OPTIONS: esmNodePathLoaderImportFlag,
      },
    })
    expect(result.status).not.toBe(0)
    expect(result.stderr.toString()).toContain('ERR_PACKAGE_PATH_NOT_EXPORTED')
  })

  test('still fails cleanly when the specifier is nowhere on NODE_PATH', () => {
    const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-esm-node-path-loader-'))
    const script = path.join(tmp, 'main.mjs')
    fs.writeFileSync(script, 'await import("truly-missing-dep")')
    const result = spawnSync(process.execPath, [script], {
      env: {
        ...process.env,
        NODE_PATH: tmp,
        NODE_OPTIONS: esmNodePathLoaderImportFlag,
      },
    })
    expect(result.status).not.toBe(0)
    expect(result.stderr.toString()).toContain('truly-missing-dep')
  })
})
