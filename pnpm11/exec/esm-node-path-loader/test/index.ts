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

  test('the fallback leaves the importer as parentURL for hooks registered after it', () => {
    // An alias resolver like the tsconfig paths hook in pnpm/pnpm#16979. NODE_OPTIONS
    // imports register before command-line ones, so the alias hook runs first and
    // reads context.parentURL after the pnpm hook's fallback fails.
    const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-esm-node-path-loader-'))
    fs.mkdirSync(path.join(tmp, 'store', 'node_modules'), { recursive: true })
    fs.mkdirSync(path.join(tmp, 'src', 'generated'), { recursive: true })
    fs.writeFileSync(path.join(tmp, 'src', 'generated', 'index.mjs'), 'export const x = 1')
    fs.writeFileSync(path.join(tmp, 'main.mjs'), `\
import { x } from '@/generated/index.mjs'
console.log(import.meta.url)
console.log('resolved', x)
`)
    // Like the pnpm hook, the alias hook falls back to module.register() where
    // module.registerHooks() is missing, so both hooks are in the same chain.
    fs.writeFileSync(path.join(tmp, 'alias-hook.mjs'), `\
import module from 'node:module'
import { resolveSync } from './alias-resolve.mjs'
if (module.registerHooks) {
  module.registerHooks({ resolve: resolveSync })
} else {
  module.register('./alias-resolve.mjs', import.meta.url)
}
`)
    fs.writeFileSync(path.join(tmp, 'alias-resolve.mjs'), `\
import { writeFileSync } from 'node:fs'
function resolveAlias (specifier, context, error) {
  if (error?.code !== 'ERR_MODULE_NOT_FOUND' || !specifier.startsWith('@/')) throw error
  writeFileSync('parent-url.txt', context.parentURL)
  return new URL('src/' + specifier.slice(2), context.parentURL).href
}
export function resolveSync (specifier, context, nextResolve) {
  try {
    return nextResolve(specifier, context)
  } catch (error) {
    return nextResolve(resolveAlias(specifier, context, error), context)
  }
}
export async function resolve (specifier, context, nextResolve) {
  try {
    return await nextResolve(specifier, context)
  } catch (error) {
    return nextResolve(resolveAlias(specifier, context, error), context)
  }
}
`)

    const result = spawnSync(process.execPath, ['--import', './alias-hook.mjs', 'main.mjs'], {
      cwd: tmp,
      env: {
        ...process.env,
        NODE_PATH: path.join(tmp, 'store', 'node_modules'),
        NODE_OPTIONS: esmNodePathLoaderImportFlag,
      },
    })
    expect(result.stderr.toString()).toBe('')
    expect(result.status).toBe(0)
    const [mainUrl, printed] = result.stdout.toString().trim().split(/\r?\n/)
    expect(fs.readFileSync(path.join(tmp, 'parent-url.txt'), 'utf8')).toBe(mainUrl)
    expect(printed).toBe('resolved 1')
  })

  test.each([
    ['with NODE_PATH', { NODE_PATH: os.tmpdir() }],
    ['without NODE_PATH', { NODE_PATH: '' }],
  ])('runs an entry point through a CommonJS require hook %s', (_name, nodePathEnv) => {
    // Mirrors how ts-node runs a .ts entry point: it registers a
    // require.extensions hook, then reruns the main module.
    const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-esm-node-path-loader-'))
    const runner = path.join(tmp, 'runner.js')
    fs.writeFileSync(runner, `\
require.extensions['.custom'] = (module, filename) => module._compile('console.log("custom-loaded")', filename)
process.argv[1] = require('node:path').join(__dirname, 'entry.custom')
require('node:module').runMain()
`)
    fs.writeFileSync(path.join(tmp, 'entry.custom'), 'not JavaScript')

    const result = spawnSync(process.execPath, [runner], {
      env: {
        ...process.env,
        ...nodePathEnv,
        NODE_OPTIONS: esmNodePathLoaderImportFlag,
      },
    })
    expect(result.stderr.toString()).toBe('')
    expect(result.status).toBe(0)
    expect(result.stdout.toString().trim()).toBe('custom-loaded')
  })

  test('still detects module syntax in a .js entry point outside a module package', () => {
    const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-esm-node-path-loader-'))
    const script = path.join(tmp, 'main.js')
    fs.writeFileSync(script, 'import path from "node:path"\nconsole.log(typeof path.join)')

    const result = spawnSync(process.execPath, [script], {
      env: {
        ...process.env,
        NODE_OPTIONS: esmNodePathLoaderImportFlag,
      },
    })
    expect(result.stderr.toString()).toBe('')
    expect(result.status).toBe(0)
    expect(result.stdout.toString().trim()).toBe('function')
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
