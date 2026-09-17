import { spawn, spawnSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import { describe, test, snapshot } from 'node:test'
import assert from 'node:assert/strict'

snapshot.setDefaultSnapshotSerializers([
  (value) => typeof value === 'string' ? `\n${value.replaceAll('\r', '')}` : JSON.stringify(value),
])
import { temporaryDirectory } from 'tempy'
import { cmdExtension } from 'cmd-extension'
import { cmdShim } from '@pnpm/bins.cmd-shim'

const describeOnWindows = process.platform === 'win32' ? describe : describe.skip
const describeOnPosix = process.platform === 'win32' ? describe.skip : describe

describeOnWindows('create a command shim for a .exe file', () => {
  test('shim files', async (t) => {
    const tempDir = temporaryDirectory()
    fs.writeFileSync(path.join(tempDir, 'foo.exe'), '', 'utf8')
    await cmdShim(path.join(tempDir, 'foo'), path.join(tempDir, 'dest'))
    const stripMarker = (s) => s.replace(/# cmd-shim-target=.*\n/, '')
    t.assert.snapshot(stripMarker(fs.readFileSync(path.join(tempDir, 'dest'), 'utf-8')))
    t.assert.snapshot(fs.readFileSync(path.join(tempDir, 'dest.cmd'), 'utf-8'))
    t.assert.snapshot(fs.readFileSync(path.join(tempDir, 'dest.ps1'), 'utf-8'))
  })
})

describeOnWindows('sh shim wrapping a .cmd target invoked from Git Bash', () => {
  // Regression for the MSYS path-translation bug: Git Bash rewrites a bare
  // `/C` argument to `C:\` before launching cmd.exe, dropping the switch
  // and leaving cmd.exe interactive. The fix escapes it as `//C` so MSYS
  // passes it through.
  test('runs the wrapped batch script and captures its output', async () => {
    const tempDir = temporaryDirectory()
    const target = path.join(tempDir, 'src.cmd')
    fs.writeFileSync(target, '@echo HELLO_FROM_CMD\r\n', 'utf8')
    const shim = path.join(tempDir, 'shim')
    await cmdShim(target, shim)

    // Invoke the sh shim via Git Bash. If `/C` survives MSYS translation,
    // cmd.exe runs src.cmd and prints HELLO_FROM_CMD; if it gets rewritten
    // to a drive path, cmd.exe starts interactively and dumps its banner
    // (`Microsoft Windows [Version ...]`) instead.
    const bash = process.env.PROGRAMFILES
      ? path.join(process.env.PROGRAMFILES, 'Git', 'bin', 'bash.exe')
      : 'bash'
    const r = spawnSync(bash, ['--noprofile', '--norc', shim], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
    })

    assert.equal(r.status, 0, `bash exited ${r.status}\nstdout: ${r.stdout}\nstderr: ${r.stderr}`)
    assert.match(r.stdout, /HELLO_FROM_CMD/, `expected script output, got:\n${r.stdout}`)
    assert.doesNotMatch(
      r.stdout,
      /Microsoft Windows \[Version/,
      'cmd.exe started interactively — the /C switch was dropped'
    )
  })
})

describeOnPosix('sh shim binstub uses exec', () => {
  // Regression for the binstub bug: without `exec`, the shell process
  // wraps the wrapped binary, so signals sent to the shim do not reach
  // the wrapped process and the binary's PID differs from the shim's
  // spawn PID. With `exec`, the shell process is replaced in place and
  // the wrapped binary inherits the shim's PID.
  test('wrapped binary inherits the shim\'s PID (proves exec replaced the shell)', async () => {
    const tempDir = temporaryDirectory()
    // process.execPath is a no-shebang native binary, so cmdShim hits the
    // non-shLongProg branch of generateShShim — the one that needs `exec`.
    const shim = path.join(tempDir, 'shim')
    await cmdShim(process.execPath, shim)

    const proc = spawn('/bin/sh', [shim, '-e', 'process.stdout.write(String(process.pid))'], {
      stdio: ['ignore', 'pipe', 'pipe'],
    })
    const shimPid = proc.pid
    let stdout = ''
    proc.stdout.on('data', (chunk) => { stdout += chunk })
    // `exit` may fire before the stdio streams have been drained. Wait for
    // `close` so stdout contains the complete PID before asserting on it.
    const exitCode = await new Promise((resolve) => proc.on('close', resolve))

    assert.equal(exitCode, 0, `shim exited ${exitCode}; stdout=${stdout}`)
    const reportedPid = Number(stdout.trim())
    assert.equal(
      reportedPid,
      shimPid,
      `expected child to inherit shim PID via exec — got reported=${reportedPid}, shim=${shimPid}`
    )
  })
})

describeOnPosix('sh shim uses POSIX runtime from PATH', () => {
  test('does not fall through to node.exe when node is only on PATH', async () => {
    const tempDir = temporaryDirectory()
    const target = path.join(tempDir, 'tool.js')
    fs.writeFileSync(target, '#!/usr/bin/env node\nconsole.log("SHIM_OK")\n', 'utf8')
    const shim = path.join(tempDir, 'tool')
    await cmdShim(target, shim, { createCmdFile: false })

    const r = spawnSync('/bin/sh', [shim], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
    })

    assert.equal(r.status, 0, `shim exited ${r.status}\nstdout: ${r.stdout}\nstderr: ${r.stderr}`)
    assert.equal(r.stdout.trim(), 'SHIM_OK')
    assert.doesNotMatch(r.stderr, /node\.exe/)
  })
})

describeOnPosix('sh shim invoked through a chain of external symlinks', () => {
  // POSIX shells set $0 to the invoked symlink, not the shim it points at,
  // so the shim must follow the chain before deriving basedir
  // (https://github.com/pnpm/pnpm/issues/13405).
  const makeShimmedTool = async (tempDir) => {
    const binDir = path.join(tempDir, 'node_modules', '.bin')
    const targetDir = path.join(tempDir, 'node_modules', 'typescript', 'bin')
    fs.mkdirSync(binDir, { recursive: true })
    fs.mkdirSync(targetDir, { recursive: true })

    const target = path.join(targetDir, 'tsc')
    fs.writeFileSync(target, '#!/bin/sh\necho "tsc-output"\n', 'utf8')
    fs.chmodSync(target, 0o755)

    const shim = path.join(binDir, 'tsc')
    await cmdShim(target, shim)
    return shim
  }

  const runShim = (cmd) => {
    const r = spawnSync(cmd, {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
    })
    assert.equal(r.status, 0, `shim exited ${r.status}\nstdout: ${r.stdout}\nstderr: ${r.stderr}`)
    return r.stdout.trim()
  }

  test('follows a chain of absolute symlinks', async () => {
    const tempDir = temporaryDirectory()
    const shim = await makeShimmedTool(tempDir)

    const hop1 = path.join(tempDir, 'symlink_hop_1')
    const hop2 = path.join(tempDir, 'symlink_hop_2')
    fs.symlinkSync(shim, hop1)
    fs.symlinkSync(hop1, hop2)

    assert.equal(runShim(hop2), 'tsc-output')
  })

  test('follows relative symlinks from another directory', async () => {
    const tempDir = temporaryDirectory()
    await makeShimmedTool(tempDir)

    // Both hops use relative targets, exercising the shim's
    // dirname-composition branch rather than the absolute one.
    const localBin = path.join(tempDir, 'usr', 'local', 'bin')
    fs.mkdirSync(localBin, { recursive: true })
    const hop1 = path.join(tempDir, 'usr', 'tsc')
    fs.symlinkSync(path.join('..', 'node_modules', '.bin', 'tsc'), hop1)
    const hop2 = path.join(localBin, 'tsc')
    fs.symlinkSync(path.join('..', '..', 'tsc'), hop2)

    assert.equal(runShim(hop2), 'tsc-output')
  })

  test('resolves symlinks when directories contain spaces', async () => {
    const tempDir = path.join(temporaryDirectory(), 'dir with spaces')
    fs.mkdirSync(tempDir, { recursive: true })
    await makeShimmedTool(tempDir)

    const hopDir = path.join(tempDir, 'link dir')
    fs.mkdirSync(hopDir)
    const hop = path.join(hopDir, 'tsc')
    fs.symlinkSync(path.join('..', 'node_modules', '.bin', 'tsc'), hop)

    assert.equal(runShim(hop), 'tsc-output')
  })

  test('runs a node-shebang bin through a symlink chain', async () => {
    const tempDir = temporaryDirectory()
    const binDir = path.join(tempDir, 'node_modules', '.bin')
    const pkgDir = path.join(tempDir, 'node_modules', 'tool')
    fs.mkdirSync(binDir, { recursive: true })
    fs.mkdirSync(pkgDir, { recursive: true })

    const target = path.join(pkgDir, 'cli.js')
    fs.writeFileSync(target, '#!/usr/bin/env node\nconsole.log("NODE_BIN_OK")\n', 'utf8')
    const shim = path.join(binDir, 'tool')
    await cmdShim(target, shim, { createCmdFile: false })

    const hop = path.join(tempDir, 'tool')
    fs.symlinkSync(path.join('node_modules', '.bin', 'tool'), hop)

    assert.equal(runShim(hop), 'NODE_BIN_OK')
  })
})

describeOnPosix('sh shim resolves its helpers off the caller\'s PATH', () => {
  // A shim runs with node_modules/.bin at the front of PATH, which is where a
  // dependency's own bins live, so a helper taken from there could report any
  // directory it liked and redirect what the shim finally execs
  // (https://github.com/pnpm/pnpm/issues/14837).
  const writeExecutable = (file, body) => {
    fs.writeFileSync(file, body, 'utf8')
    fs.chmodSync(file, 0o755)
  }

  // A shimmed tool plus a relative symlink to it in the same directory, so the
  // walk composes a directory with the link target instead of taking one
  // straight from readlink.
  const makeShimmedTool = async (tempDir) => {
    const binDir = path.join(tempDir, 'node_modules', '.bin')
    const target = path.join(tempDir, 'node_modules', 'typescript', 'bin', 'tsc.js')
    fs.mkdirSync(binDir, { recursive: true })
    fs.mkdirSync(path.dirname(target), { recursive: true })
    fs.writeFileSync(target, 'console.log("tsc-output")\n', 'utf8')
    // A dependency can declare a bin named node.exe, and the shim's basedir is
    // the directory those bins land in. Only a lying uname reaches it.
    writeExecutable(path.join(binDir, 'node.exe'), '#!/bin/sh\necho hijacked\n')

    const shim = path.join(binDir, 'tsc')
    await cmdShim(target, shim, { createCmdFile: false })
    fs.symlinkSync('tsc', path.join(binDir, 'tsc-link'))
    return binDir
  }

  // Write the tree the decoys point at, and the decoys, returning the directory
  // to put at the front of PATH. Each decoy answers with what its real
  // counterpart would be asked for, so any one of them alone is enough to
  // redirect the shim.
  const plantHijackTreeAndDecoys = (tempDir) => {
    const hijack = path.join(tempDir, 'hijack', 'node_modules')
    const hijackBin = path.join(hijack, '.bin')
    const hijackTarget = path.join(hijack, 'typescript', 'bin', 'tsc.js')
    fs.mkdirSync(hijackBin, { recursive: true })
    fs.mkdirSync(path.dirname(hijackTarget), { recursive: true })
    fs.writeFileSync(hijackTarget, 'console.log("hijacked")\n', 'utf8')

    const decoyDir = path.join(tempDir, 'decoy')
    fs.mkdirSync(decoyDir)
    const answer = (p) => `#!/bin/sh\necho '${p}'\n`
    for (const helper of ['readlink', 'sed', 'printf']) {
      writeExecutable(path.join(decoyDir, helper), answer(path.join(hijackBin, 'tsc')))
    }
    writeExecutable(path.join(decoyDir, 'dirname'), answer(hijackBin))
    writeExecutable(path.join(decoyDir, 'uname'), '#!/bin/sh\necho MINGW64_NT-10.0\n')
    return decoyDir
  }

  const runWithDecoys = (tempDir, cmd, args, cwd) => {
    const decoyDir = plantHijackTreeAndDecoys(tempDir)
    const r = spawnSync(cmd, args, {
      cwd,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
      env: {
        ...process.env,
        PATH: [decoyDir, path.dirname(process.execPath), process.env.PATH].join(path.delimiter),
      },
    })
    assert.equal(r.status, 0, `shim exited ${r.status}\nstdout: ${r.stdout}\nstderr: ${r.stderr}`)
    assert.equal(r.stdout.trim(), 'tsc-output', 'the shim took a helper from the caller\'s PATH')
  }

  test('reaches its target with decoy readlink, dirname, sed, and uname first on PATH', async () => {
    const tempDir = temporaryDirectory()
    const binDir = await makeShimmedTool(tempDir)

    runWithDecoys(tempDir, path.join(binDir, 'tsc-link'), [])
  })

  // The kernel and execvp hand the interpreter the path they resolved, so $0 is
  // bare only when a shell is given the name itself.
  test('reaches its target when sh receives a bare name', async () => {
    const tempDir = temporaryDirectory()
    const binDir = await makeShimmedTool(tempDir)

    runWithDecoys(tempDir, 'sh', ['tsc-link'], binDir)
  })
})

describeOnPosix('sh shim converts a Windows-form path', () => {
  // The header converts backslashes to slashes with `sed`. A POSIX `echo` would
  // turn the `\n` of `\node_modules` into a newline and the `\t` of `\tsc` into
  // a tab before `sed` ever saw them (https://github.com/pnpm/pnpm/issues/14867).
  test('keeps the backslashes until sed converts them', async () => {
    const tempDir = temporaryDirectory()
    const target = path.join(tempDir, 'tool.js')
    fs.writeFileSync(target, '#!/usr/bin/env node\nconsole.log("SHIM_OK")\n', 'utf8')
    const shim = path.join(tempDir, 'tool')
    await cmdShim(target, shim, { createCmdFile: false })

    const conversion = fs.readFileSync(shim, 'utf8')
      .split('\n')
      .find((line) => line.startsWith('basedir=$('))
    assert.ok(conversion, 'the header must assign basedir from the shim path')
    // POSIX echo backslash handling is implementation-defined. A shell that
    // preserves backslashes can make an echo-based header pass the path
    // assertion below, so also require the printf conversion form.
    assert.ok(
      conversion.includes(String.raw`command -p printf '%s\n' "$link"`),
      'the basedir conversion must use command -p printf so backslashes stay literal'
    )

    const script = `link='C:\\node_modules\\.bin\\tsc'\n${conversion}\nprintf '%s' "$basedir"`
    const r = spawnSync('/bin/sh', ['-c', script], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
    })

    assert.equal(r.status, 0, `sh exited ${r.status}\nstderr: ${r.stderr}`)
    assert.equal(r.stdout, 'C:/node_modules/.bin/tsc')
  })
})

describeOnPosix('relocatable project shims', () => {
  for (const externalTarget of [false, true]) {
    test(`moves scoped NODE_PATH and runtime paths with an ${externalTarget ? 'external' : 'internal'} target`, async (t) => {
      const tempDir = temporaryDirectory()
      t.after(() => fs.rmSync(tempDir, { recursive: true, force: true }))
      const project = path.join(tempDir, 'project')
      const moved = path.join(tempDir, 'moved project')
      const bins = path.join(project, 'node_modules', '.bin')
      const special = 'paths $value `echo injected` "quoted" \\literal'
      const targetDir = path.join(externalTarget ? tempDir : project, special.replaceAll('\\', ''))
      const target = path.join(targetDir, 'tool.js')
      const runtime = path.join(project, 'runtime node')
      const nodePaths = [path.join(project, special, 'node_modules'), path.join(tempDir, 'external $literal')]
      fs.mkdirSync(bins, { recursive: true })
      fs.mkdirSync(targetDir, { recursive: true })
      fs.writeFileSync(target, '#!/usr/bin/env node\nconsole.log(JSON.stringify({ paths: process.env.NODE_PATH.split(":"), args: process.argv.slice(2) }))\n')
      fs.symlinkSync(process.execPath, runtime)
      const shim = path.join(bins, 'tool')
      await cmdShim(target, shim, {
        relocatableRoot: project,
        nodePath: nodePaths,
        nodeExecPath: runtime,
        createPwshFile: false,
      })
      const content = fs.readFileSync(shim, 'utf8')
      assert.ok(!content.includes(project))
      if (externalTarget) {
        assert.ok(content.includes('cmd-shim-target=' + target.split('\\').join('/')))
        assert.ok(content.includes('$basedir_abs/../../../'))
      }
      fs.renameSync(project, moved)
      const alias = path.join(tempDir, 'alias')
      fs.symlinkSync(path.join(moved, 'node_modules'), alias)
      const invokedShims = [path.join(moved, 'node_modules', '.bin', 'tool')]
      if (!externalTarget) invokedShims.push(path.join(alias, '.bin', 'tool'))
      for (const invokedShim of invokedShims) {
        const result = spawnSync('/bin/sh', [invokedShim, 'argument with spaces'], {
          cwd: tempDir,
          encoding: 'utf8',
          env: { ...process.env, NODE_PATH: '/inherited/path' },
        })
        assert.equal(result.status, 0, result.stderr)
        const output = JSON.parse(result.stdout)
        assert.deepEqual(output.paths.map(entry => path.resolve(entry)), [
          path.join(moved, special, 'node_modules'),
          nodePaths[1],
          '/inherited/path',
        ])
        assert.deepEqual(output.args, ['argument with spaces'])
      }
    })
  }
})

test('shims outside the relocatable root keep their existing content', async (t) => {
  const tempDir = temporaryDirectory()
  t.after(() => fs.rmSync(tempDir, { recursive: true, force: true }))
  const target = path.join(tempDir, 'tool.js')
  const shim = path.join(tempDir, 'tool')
  fs.writeFileSync(target, '#!/usr/bin/env node\n')
  const opts = { createCmdFile: true, nodeExecPath: process.execPath, nodePath: [path.join(tempDir, 'node_modules')] }
  await cmdShim(target, shim, opts)
  const files = [shim, shim + cmdExtension, shim + '.ps1']
  const content = files.map(file => fs.readFileSync(file, 'utf8'))
  await cmdShim(target, shim, { ...opts, relocatableRoot: path.join(tempDir, 'project') })
  assert.deepEqual(files.map(file => fs.readFileSync(file, 'utf8')), content)
})

describeOnPosix('relocatable shims generated through directory symlinks', () => {
  for (const terminal of [false, true]) {
    test(`resolves a ${terminal ? 'terminal' : 'interior'} bin symlink and missing NODE_PATH tails`, async (t) => {
      const tempDir = temporaryDirectory()
      t.after(() => fs.rmSync(tempDir, { recursive: true, force: true }))
      const project = path.join(tempDir, 'project')
      const physical = path.join(project, 'deep', 'physical')
      fs.mkdirSync(physical, { recursive: true })
      const alias = path.join(project, 'alias')
      fs.symlinkSync('deep/physical', alias)
      const bins = terminal ? alias : path.join(alias, '.bin')
      const target = path.join(project, 'tool.js')
      fs.writeFileSync(target, '#!/usr/bin/env node\nconsole.log(require("relocation-probe"))\n')
      const nodePathAlias = path.join(project, 'node-path-alias')
      fs.symlinkSync(physical, nodePathAlias)
      const lexicalModules = path.join(nodePathAlias, 'missing', 'node_modules')
      const shim = path.join(bins, 'tool')
      await cmdShim(target, shim, { relocatableRoot: project, nodePath: [lexicalModules], createPwshFile: false })
      const probeDir = path.join(lexicalModules, 'relocation-probe')
      fs.mkdirSync(probeDir, { recursive: true })
      fs.writeFileSync(path.join(probeDir, 'index.js'), 'module.exports = "found"\n')
      const moved = path.join(tempDir, 'moved')
      fs.renameSync(project, moved)
      const result = spawnSync('/bin/sh', [path.join(moved, path.relative(project, shim))], {
        encoding: 'utf8',
        cwd: tempDir,
        env: { ...process.env, NODE_PATH: '', NODE_OPTIONS: '' },
      })
      assert.equal(result.status, 0, result.stderr)
      assert.equal(result.stdout, 'found\n')
    })
  }
})

describeOnPosix('unresolvable relocatable paths', () => {
  test('keeps dangling optional NODE_PATH entries without blocking the executable', async (t) => {
    const tempDir = temporaryDirectory()
    t.after(() => fs.rmSync(tempDir, { recursive: true, force: true }))
    const target = path.join(tempDir, 'tool.js')
    fs.writeFileSync(target, '#!/usr/bin/env node\nconsole.log(process.env.NODE_PATH)\n')
    const dangling = path.join(tempDir, 'dangling')
    fs.symlinkSync('missing', dangling)
    const shim = path.join(tempDir, '.bin', 'tool')
    await cmdShim(target, shim, {
      relocatableRoot: tempDir,
      nodePath: [dangling],
      createPwshFile: false,
    })
    const result = spawnSync('/bin/sh', [shim], {
      encoding: 'utf8',
      env: { ...process.env, NODE_PATH: '', NODE_OPTIONS: '' },
    })
    assert.equal(result.status, 0, result.stderr)
    assert.equal(result.stdout.trim(), dangling)
  })

  test('rejects an unresolvable required root', async (t) => {
    const tempDir = temporaryDirectory()
    t.after(() => fs.rmSync(tempDir, { recursive: true, force: true }))
    const target = path.join(tempDir, 'tool.js')
    fs.writeFileSync(target, '#!/usr/bin/env node\n')
    const dangling = path.join(tempDir, 'dangling')
    fs.symlinkSync('missing', dangling)
    await assert.rejects(cmdShim(target, path.join(tempDir, '.bin', 'tool'), {
      relocatableRoot: dangling,
      createPwshFile: false,
    }), { code: 'ENOENT' })
  })
})

describeOnPosix('NODE_PATH entries outside physical relocation scope', () => {
  test('resolves escaping aliases but preserves external aliases and legacy relative entries', async (t) => {
    const tempDir = temporaryDirectory()
    t.after(() => fs.rmSync(tempDir, { recursive: true, force: true }))
    const project = path.join(tempDir, 'project')
    const outside = path.join(tempDir, 'outside')
    fs.mkdirSync(project)
    fs.mkdirSync(outside)
    const alias = path.join(project, 'external-alias')
    fs.symlinkSync(outside, alias)
    const outsideAlias = path.join(tempDir, 'outside-alias')
    fs.symlinkSync(outside, outsideAlias)
    const target = path.join(project, 'tool.js')
    const shim = path.join(project, '.bin', 'tool')
    fs.writeFileSync(target, '#!/usr/bin/env node\n')
    await cmdShim(target, shim, { relocatableRoot: project, nodePath: [alias, outsideAlias, 'relative-modules'], createPwshFile: false })
    assert.ok(fs.readFileSync(shim, 'utf8').includes(`export NODE_PATH="${outside}:${outsideAlias}:relative-modules"`))
    await cmdShim(target, shim, { relocatableRoot: project, nodePath: '', createPwshFile: false })
    assert.ok(!fs.readFileSync(shim, 'utf8').includes('export NODE_PATH='))
  })
})

describeOnPosix('external executable aliases', () => {
  test('follows retargeted external source and Node runtime directories', async (t) => {
    const tempDir = temporaryDirectory()
    t.after(() => fs.rmSync(tempDir, { recursive: true, force: true }))
    const project = path.join(tempDir, 'project')
    fs.mkdirSync(project)
    for (const version of ['one', 'two']) {
      const dir = path.join(tempDir, version)
      fs.mkdirSync(dir)
      fs.writeFileSync(path.join(dir, 'tool.js'), `#!/usr/bin/env node\nconsole.log('${version}')\n`)
      fs.writeFileSync(path.join(dir, 'node'), `#!/bin/sh\nprintf '${version}\\n'\n`, { mode: 0o755 })
    }
    const alias = path.join(tempDir, 'current')
    fs.symlinkSync('one', alias)
    const target = path.join(project, 'tool.js')
    fs.writeFileSync(target, '#!/usr/bin/env node\n')
    await cmdShim(path.join(alias, 'tool.js'), path.join(project, 'source'), { relocatableRoot: project, createPwshFile: false })
    await cmdShim(target, path.join(project, 'runtime'), { relocatableRoot: project, nodeExecPath: path.join(alias, 'node'), createPwshFile: false })
    fs.unlinkSync(alias)
    fs.symlinkSync('two', alias)
    for (const name of ['source', 'runtime']) {
      const result = spawnSync('/bin/sh', [path.join(project, name)], { encoding: 'utf8', env: { ...process.env, NODE_OPTIONS: '' } })
      assert.equal(result.status, 0, result.stderr)
      assert.equal(result.stdout, 'two\n')
    }
    fs.unlinkSync(alias)
    fs.symlinkSync('missing', alias)
    await cmdShim(target, path.join(project, 'unavailable-runtime'), { relocatableRoot: project, nodeExecPath: path.join(alias, 'node'), createPwshFile: false })
    assert.ok(fs.readFileSync(path.join(project, 'unavailable-runtime'), 'utf8').includes(path.join(alias, 'node')))
    const internalRuntime = path.join(project, 'internal-runtime')
    fs.symlinkSync('missing', internalRuntime)
    await assert.rejects(cmdShim(target, path.join(project, 'invalid-runtime'), {
      relocatableRoot: project,
      nodeExecPath: path.join(internalRuntime, 'node'),
      createPwshFile: false,
    }), { code: 'ENOENT' })
  })
})
