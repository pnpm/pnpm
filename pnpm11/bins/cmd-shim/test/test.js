import { describe, test, before, snapshot } from 'node:test'
import assert from 'node:assert/strict'

snapshot.setDefaultSnapshotSerializers([
  (value) => typeof value === 'string' ? `\n${value.replaceAll('\r', '')}` : JSON.stringify(value),
])
import path from 'node:path'
import { cmdExtension } from 'cmd-extension'
import { fixtures, fixtures2, fs, setupFixtures } from './setup.js'
import { cmdShim, isShimPointingAt } from '@pnpm/bins.cmd-shim'

/**
 * @param {import('node:test').TestContext} t
 * @param {string} fileName
 * @param {'\n' | '\r\n'} lineEnding
 */
async function testFile (t, fileName, lineEnding = '\n') {
  await t.test(path.basename(fileName).toLowerCase(), async (t) => {
    const invalidLineEnding = lineEnding === '\r\n' ? /$(?<!\r)\n/ugm : /$\r\n/ugm
    let content = await fs.promises.readFile(fileName, 'utf8')

    assert.equal(invalidLineEnding.test(content), false, 'unexpected line ending')
    // Normalize cmd extension casing for cross-platform snapshot consistency
    if (cmdExtension !== '.cmd') {
      content = content.replaceAll(cmdExtension, '.cmd')
    }
    // Strip the target marker line — it contains a platform-dependent absolute path.
    // The marker is tested directly by the isShimPointingAt tests.
    content = content.replace(/# cmd-shim-target=.*\n/, '')
    t.assert.snapshot(content)
  })
}

describe('isShimPointingAt', () => {
  const src = path.resolve(fixtures, 'src.exe')
  const to = path.resolve(fixtures, 'exe.shim')
  before(async () => {
    await setupFixtures()
    return cmdShim(src, to, { createCmdFile: false, fs })
  })

  test('returns true for the correct target', async () => {
    const content = await fs.promises.readFile(to, 'utf8')
    assert.equal(isShimPointingAt(content, src), true)
  })

  test('returns false for a different target', async () => {
    const content = await fs.promises.readFile(to, 'utf8')
    assert.equal(isShimPointingAt(content, '/wrong/path.exe'), false)
  })

  test('returns false for a subdirectory prefix of the target', async () => {
    const content = await fs.promises.readFile(to, 'utf8')
    // src without the last path segment — must not match
    assert.equal(isShimPointingAt(content, path.dirname(src)), false)
  })
})

describe('no cmd file', () => {
  const src = path.resolve(fixtures, 'src.exe')
  const to = path.resolve(fixtures, 'exe.shim')
  before(async () => {
    await setupFixtures()
    return cmdShim(src, to, { createCmdFile: false, fs })
  })

  test('shim files', async (t) => {
    await testFile(t, to)
    assert.throws(() => fs.readFileSync(`${to}.cmd`, 'utf8'), /no such file or directory/)
    await testFile(t, `${to}.ps1`)
  })
})

describe('no shebang', () => {
  const src = path.resolve(fixtures, 'src.exe')
  const to = path.resolve(fixtures, 'exe.shim')
  before(async () => {
    await setupFixtures()
    return cmdShim(src, to, { createCmdFile: true, fs })
  })

  test('shim files', async (t) => {
    await testFile(t, to)
    await testFile(t, `${to}${cmdExtension}`, '\r\n')
    await testFile(t, `${to}.ps1`)
  })
})

describe('env shebang', () => {
  const src = path.resolve(fixtures, 'src.env')
  const to = path.resolve(fixtures, 'env.shim')
  before(async () => {
    await setupFixtures()
    return cmdShim(src, to, { createCmdFile: true, fs })
  })

  test('shim files', async (t) => {
    await testFile(t, to)
    await testFile(t, `${to}${cmdExtension}`, '\r\n')
    await testFile(t, `${to}.ps1`)
  })
})

describe('env shebang with PATH extending', () => {
  const src = path.resolve(fixtures, 'src.env')
  const to = path.resolve(fixtures, 'env.shim')
  before(async () => {
    await setupFixtures()
    return cmdShim(src, to, { prependToPath: '/add-to-path', createCmdFile: true, fs })
  })

  test('shim files', async (t) => {
    await testFile(t, to)
    await testFile(t, `${to}${cmdExtension}`, '\r\n')
    await testFile(t, `${to}.ps1`)
  })
})

describe('env shebang with NODE_PATH', () => {
  const src = path.resolve(fixtures, 'src.env')
  const to = path.resolve(fixtures, 'env.shim')
  before(async () => {
    await setupFixtures()
    return cmdShim(src, to, { nodePath: ['/john/src/node_modules', '/bin/node/node_modules'], createCmdFile: true, fs })
  })

  test('shim files', async (t) => {
    await testFile(t, to)
    await testFile(t, `${to}${cmdExtension}`, '\r\n')
    await testFile(t, `${to}.ps1`)
  })
})

describe('env shebang with no NODE_PATH', () => {
  const src = path.resolve(fixtures, 'src.env')
  const to = path.resolve(fixtures, 'env.shim')
  before(async () => {
    await setupFixtures()
    return cmdShim(src, to, { nodePath: [], createCmdFile: true, fs })
  })

  test('shim files', async (t) => {
    await testFile(t, to)
    await testFile(t, `${to}${cmdExtension}`, '\r\n')
    await testFile(t, `${to}.ps1`)
  })
})

describe('env shebang with default args', () => {
  const src = path.resolve(fixtures, 'src.env')
  const to = path.resolve(fixtures, 'env.shim')
  before(async () => {
    await setupFixtures()
    return cmdShim(src, to, { preserveSymlinks: true, createCmdFile: true, fs })
  })

  test('shim files', async (t) => {
    await testFile(t, to)
    await testFile(t, `${to}${cmdExtension}`, '\r\n')
    await testFile(t, `${to}.ps1`)
  })
})

describe('env shebang with args', () => {
  const src = path.resolve(fixtures, 'src.env.args')
  const to = path.resolve(fixtures, 'env.args.shim')
  before(async () => {
    await setupFixtures()
    return cmdShim(src, to, { createCmdFile: true, fs })
  })

  test('shim files', async (t) => {
    await testFile(t, to)
    await testFile(t, `${to}${cmdExtension}`, '\r\n')
    await testFile(t, `${to}.ps1`)
  })
})

describe('explicit shebang', () => {
  const src = path.resolve(fixtures, 'src.sh')
  const to = path.resolve(fixtures, 'sh.shim')
  before(async () => {
    await setupFixtures()
    return cmdShim(src, to, { createCmdFile: true, fs })
  })

  test('shim files', async (t) => {
    await testFile(t, to)
    await testFile(t, `${to}${cmdExtension}`, '\r\n')
    await testFile(t, `${to}.ps1`)
  })
})

describe('explicit shebang with args', () => {
  const src = path.resolve(fixtures, 'src.sh.args')
  const to = path.resolve(fixtures, 'sh.args.shim')
  before(async () => {
    await setupFixtures()
    return cmdShim(src, to, { createCmdFile: true, fs })
  })

  test('shim files', async (t) => {
    await testFile(t, to)
    await testFile(t, `${to}${cmdExtension}`, '\r\n')
    await testFile(t, `${to}.ps1`)
  })
})

describe('explicit shebang with prog args', () => {
  const src = path.resolve(fixtures, 'src.sh.args')
  const to = path.resolve(fixtures, 'sh.args.shim')
  before(async () => {
    await setupFixtures()
    return cmdShim(src, to, { createCmdFile: true, progArgs: ['hello'], fs })
  })

  test('shim files', async (t) => {
    await testFile(t, to)
    await testFile(t, `${to}${cmdExtension}`, '\r\n')
    await testFile(t, `${to}.ps1`)
  })
})

describe('custom node executable', () => {
  const src = path.resolve(fixtures, 'src.env')
  const to = path.resolve(fixtures, 'env.shim')
  before(async () => {
    await setupFixtures()
    return cmdShim(src, to, { createCmdFile: true, nodeExecPath: '/.pnpm/nodejs/16.0.0/node', fs })
  })

  test('shim files', async (t) => {
    await testFile(t, to)
    await testFile(t, `${to}${cmdExtension}`, '\r\n')
    await testFile(t, `${to}.ps1`)
  })
})

const describeOnWindows = process.platform === 'win32' ? describe : describe.skip

describeOnWindows('explicit shebang with args, linking to another drive on Windows', () => {
  const src = path.resolve(fixtures2, 'src.sh.args')
  const to = path.resolve(fixtures, 'sh.args.shim')
  before(async () => {
    await setupFixtures()
    return cmdShim(src, to, { createCmdFile: true, fs })
  })

  test('shim files', async (t) => {
    await testFile(t, to)
    await testFile(t, `${to}${cmdExtension}`, '\r\n')
    await testFile(t, `${to}.ps1`)
  })
})

describe('shebang with -S', () => {
  const src = path.resolve(fixtures, 'from.env.S')
  const to = path.resolve(fixtures, 'from.env.S.shim')
  before(async () => {
    await setupFixtures()
    return cmdShim(src, to, { createCmdFile: true, fs })
  })

  test('shim files', async (t) => {
    await testFile(t, to)
    await testFile(t, `${to}${cmdExtension}`, '\r\n')
    await testFile(t, `${to}.ps1`)
  })
})

describe('batch script', () => {
  const src = path.resolve(fixtures, 'src.bat')
  const to = path.resolve(fixtures, 'bat.shim')
  before(async () => {
    await setupFixtures()
    return cmdShim(src, to, { createCmdFile: true, fs })
  })

  test('shim files', async (t) => {
    await testFile(t, to)
    await testFile(t, `${to}${cmdExtension}`, '\r\n')
    await testFile(t, `${to}.ps1`)
  })
})

test('relocatable paths use the supplied filesystem', { skip: process.platform === 'win32' }, async () => {
  const root = path.join(fixtures, 'relocatable')
  const physical = path.join(root, 'deep', 'bins')
  await fs.promises.mkdir(physical, { recursive: true })
  await fs.promises.symlink('deep/bins', path.join(root, 'alias'))
  const target = path.join(root, 'tool.js')
  await fs.promises.writeFile(target, '#!/usr/bin/env node\n')
  await cmdShim(target, path.join(root, 'alias', 'tool'), {
    fs,
    relocatableRoot: root,
    nodePath: [path.join(root, 'missing', 'node_modules')],
    createPwshFile: false,
  })
  const content = await fs.promises.readFile(path.join(physical, 'tool'), 'utf8')
  assert.ok(content.includes('export NODE_PATH="$basedir_abs/../../missing/node_modules"'))
})
