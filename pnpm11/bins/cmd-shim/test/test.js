import { describe, test, before, snapshot } from 'node:test'
import assert from 'node:assert/strict'

snapshot.setDefaultSnapshotSerializers([
  (value) => typeof value === 'string' ? `\n${value.replaceAll('\r', '')}` : JSON.stringify(value),
])
import path from 'node:path'
import { cmdExtension } from 'cmd-extension'
import { fixtures, fixtures2, fs, setupFixtures } from './setup.js'
import {
  cmdShim,
  cmdShimIfExists,
  isShimForMissingTarget,
  isShimNodePath,
  isShimPointingAt,
  readShNodePath,
} from '@pnpm/bins.cmd-shim'

/**
 * @param {import('node:test').TestContext} t
 * @param {string} fileName
 * @param {'\n' | '\r\n'} lineEnding
 * @param {string} name The subtest name, which keys the snapshot.
 */
async function testFile (t, fileName, lineEnding = '\n', name = path.basename(fileName).toLowerCase()) {
  await t.test(name, async (t) => {
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

describe('missing source', () => {
  const to = path.resolve(fixtures, 'missing.shim')
  before(setupFixtures)

  test('infers the runtime from the extension', async () => {
    const src = path.resolve(fixtures, 'dist', 'missing.js')
    await cmdShim(src, to, { createCmdFile: true, fs })
    assert.match(fs.readFileSync(to, 'utf8'), /\n +exec node +"\$basedir\/dist\/missing\.js" "\$@"\n/)
    assert.match(fs.readFileSync(`${to}${cmdExtension}`, 'utf8'), /\n +node +"%~dp0\\dist\\missing\.js" %\*/)
  })

  test('runs a source without a known extension directly', async () => {
    const src = path.resolve(fixtures, 'missing')
    await cmdShim(src, to, { createCmdFile: false, fs })
    const content = fs.readFileSync(to, 'utf8')
    assert.match(content, /\nexec "\$basedir\/missing" +"\$@"\n/)
    assert.doesNotMatch(content, /exec node/)
  })

  test('cmdShimIfExists writes no shim', async () => {
    const skipped = path.resolve(fixtures, 'if-exists.shim')
    await cmdShimIfExists(path.resolve(fixtures, 'missing'), skipped, { createCmdFile: true, fs })
    assert.equal(fs.existsSync(skipped), false)
    assert.equal(fs.existsSync(`${skipped}${cmdExtension}`), false)
  })

  test('marks the shim as written for a missing target', async () => {
    await cmdShim(path.resolve(fixtures, 'missing'), to, { createCmdFile: false, fs })
    assert.equal(isShimForMissingTarget(fs.readFileSync(to, 'utf8')), true)

    await cmdShim(path.resolve(fixtures, 'src.env'), to, { createCmdFile: false, fs })
    assert.equal(isShimForMissingTarget(fs.readFileSync(to, 'utf8')), false)
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
    // A shim written on Windows picks the NODE_PATH form when it runs.
    await testFile(t, to, '\n', process.platform === 'win32' ? 'env.shim (windows)' : 'env.shim')
    await testFile(t, `${to}${cmdExtension}`, '\r\n')
    await testFile(t, `${to}.ps1`)
  })
})

describe('paths containing %', () => {
  const src = path.resolve(fixtures, '50% off', 'src.env')
  const to = path.resolve(fixtures, 'percent.shim')
  before(async () => {
    await setupFixtures()
    await fs.promises.mkdir(path.dirname(src), { recursive: true })
    await fs.promises.writeFile(src, '#!/usr/bin/env node\nconsole.log(/hi/)\n')
    return cmdShim(src, to, {
      nodePath: ['/50% off/node_modules'],
      prependToPath: '/50% off/bin',
      nodeExecPath: '/50% off/node',
      createCmdFile: true,
      fs,
    })
  })

  test('are escaped in the cmd shim', async () => {
    const content = await fs.promises.readFile(`${to}${cmdExtension}`, 'utf8')
    assert.ok(content.includes('@SET "NODE_PATH=\\50%% off\\node_modules;%NODE_PATH%"'), content)
    assert.ok(content.includes('"%~dp0\\50%% off\\src.env"'), content)
    assert.ok(content.includes('@SET "PATH=\\50%% off\\bin:%PATH%"'), content)
    assert.ok(content.includes('"/50%% off/node"'), content)
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

describe('readShNodePath & isShimNodePath', () => {
  test('extracts posix new_node_path with single quote escapes from Windows shim', () => {
    const shim = `
case \`command -p uname -a\` in
  *CYGWIN*|*MINGW*|*MSYS*)
    exe=".exe"
    msys="true"
  ;;
esac

if [ -n "$msys" ]; then
  new_node_path='C:\\foo\\bar'
  node_path_sep=';'
else
  new_node_path='/mnt/c/it'\\''s/path'
  node_path_sep=':'
fi
if [ -z "$NODE_PATH" ]; then
  export NODE_PATH="$new_node_path"
fi
`
    assert.equal(readShNodePath(shim), "/mnt/c/it's/path")
    assert.equal(isShimNodePath(shim, { first: "/mnt/c/it's/path" }), true)
  })
})

