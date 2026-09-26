import { spawn, spawnSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import { describe, test, snapshot } from 'node:test'
import assert from 'node:assert/strict'

snapshot.setDefaultSnapshotSerializers([
  (value) => typeof value === 'string' ? `\n${value.replaceAll('\r', '')}` : JSON.stringify(value),
])
import { temporaryDirectory } from 'tempy'
import { cmdShim } from '@pnpm/bins.cmd-shim'

const describeOnWindows = process.platform === 'win32' ? describe : describe.skip
const describeOnPosix = process.platform === 'win32' ? describe.skip : describe

describeOnWindows('CMD shims with Unicode paths', () => {
  for (const codepage of [437, 936, 65001]) {
    for (const exitCode of [0, 7]) {
      test(`runs a Unicode target under CP ${codepage} and preserves exit ${exitCode}`, async () => {
        const tempDir = temporaryDirectory()
        try {
          const targetDir = path.join(tempDir, '工具')
          fs.mkdirSync(targetDir)
          const target = path.join(targetDir, 'cli.js')
          fs.writeFileSync(target, '#!/usr/bin/env node\nconsole.log(JSON.stringify(process.argv.slice(2)))\nprocess.exit(Number(process.argv[4]))\n', 'utf8')
          await cmdShim(target, path.join(tempDir, 'shim'))
          fs.writeFileSync(path.join(tempDir, 'run.cmd'), [
            '@echo off',
            '@for /f "tokens=2 delims=:" %%a in (\'chcp\') do @set "original_codepage=%%a"',
            `@chcp ${codepage}>nul`,
            `@call shim.cmd "argument with spaces" "a&b" ${exitCode}`,
            '@set "shim_exit=%errorlevel%"',
            '@chcp',
            '@chcp %original_codepage%>nul',
            '@exit /b %shim_exit%',
          ].join('\r\n') + '\r\n', 'utf8')
          const result = spawnSync('cmd.exe', ['/d', '/c', 'run.cmd'], {
            cwd: tempDir,
            encoding: 'utf8',
            windowsHide: true,
            timeout: 30000,
          })
          assert.equal(result.status, exitCode, `stdout: ${result.stdout}\nstderr: ${result.stderr}`)
          const [args, restoredCodepage] = result.stdout.trim().split(/\r?\n/)
          assert.deepEqual(JSON.parse(args), ['argument with spaces', 'a&b', String(exitCode)])
          assert.match(restoredCodepage, new RegExp(`\\b${codepage}\\b`))
          if (codepage === 936) {
            fs.writeFileSync(path.join(tempDir, 'exit-only.cmd'), `@shim.cmd "argument with spaces" "a&b" ${exitCode}\r\n`, 'utf8')
            const shadowedResult = spawnSync('cmd.exe', ['/d', '/c', 'exit-only.cmd'], {
              cwd: tempDir,
              env: { ...process.env, ERRORLEVEL: '99' },
              encoding: 'utf8',
              windowsHide: true,
              timeout: 30000,
            })
            assert.equal(shadowedResult.status, exitCode, `stdout: ${shadowedResult.stdout}\nstderr: ${shadowedResult.stderr}`)
          }
        } finally {
          fs.rmSync(tempDir, { recursive: true, force: true })
        }
      })
    }
  }
})

describeOnWindows('PowerShell shims with Unicode paths', () => {
  for (const exitCode of [0, 7]) {
    test(`runs a Unicode target under Windows PowerShell and preserves exit ${exitCode}`, async () => {
      const tempDir = temporaryDirectory()
      try {
        const targetDir = path.join(tempDir, '工具')
        fs.mkdirSync(targetDir)
        const target = path.join(targetDir, 'cli.js')
        fs.writeFileSync(target, '#!/usr/bin/env node\nconsole.log(JSON.stringify(process.argv.slice(2)))\nprocess.exit(Number(process.argv[3]))\n', 'utf8')
        await cmdShim(target, path.join(tempDir, 'shim'))
        const shim = path.join(tempDir, 'shim.ps1')
        assert.ok(fs.readFileSync(shim).subarray(0, 3).equals(Buffer.from([0xEF, 0xBB, 0xBF])), 'the shim must start with a UTF-8 BOM')
        const result = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', shim, 'argument with spaces', String(exitCode)], {
          encoding: 'utf8',
          windowsHide: true,
          timeout: 30000,
        })
        assert.equal(result.status, exitCode, `stdout: ${result.stdout}\nstderr: ${result.stderr}`)
        assert.deepEqual(JSON.parse(result.stdout.trim()), ['argument with spaces', String(exitCode)])
      } finally {
        fs.rmSync(tempDir, { recursive: true, force: true })
      }
    })
  }

  test('leaves an ASCII-only shim without a BOM', async () => {
    const tempDir = temporaryDirectory()
    try {
      const target = path.join(tempDir, 'cli.js')
      fs.writeFileSync(target, '#!/usr/bin/env node\n', 'utf8')
      await cmdShim(target, path.join(tempDir, 'shim'))
      assert.ok(fs.readFileSync(path.join(tempDir, 'shim.ps1'), 'utf8').startsWith('#!/usr/bin/env pwsh\n'))
    } finally {
      fs.rmSync(tempDir, { recursive: true, force: true })
    }
  })
})

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

describeOnWindows('sh shim NODE_PATH invoked from Git Bash', () => {
  // Regression for pnpm/pnpm#3360: a shim written outside MSYS used to bake
  // NODE_PATH as /mnt/c/..., which MSYS turns into a path under the Git
  // install directory before node starts. The shim now passes the Windows form.
  const bash = process.env.PROGRAMFILES
    ? path.join(process.env.PROGRAMFILES, 'Git', 'bin', 'bash.exe')
    : 'bash'
  const runShim = async (inheritedNodePath) => {
    const tempDir = temporaryDirectory()
    const target = path.join(tempDir, 'print.js')
    fs.writeFileSync(target, '#!/usr/bin/env node\nprocess.stdout.write(process.env.NODE_PATH)\n', 'utf8')
    const nodePath = path.join(tempDir, 'node_modules')
    const shim = path.join(tempDir, 'shim')
    await cmdShim(target, shim, { nodePath: [nodePath] })
    const env = Object.fromEntries(Object.entries(process.env).filter(([name]) => name.toUpperCase() !== 'NODE_PATH'))
    if (inheritedNodePath != null) env.NODE_PATH = inheritedNodePath
    const r = spawnSync(bash, ['--noprofile', '--norc', shim], { encoding: 'utf8', env })
    assert.equal(r.status, 0, `bash exited ${r.status}\nstdout: ${r.stdout}\nstderr: ${r.stderr}`)
    return { nodePath, stdout: r.stdout }
  }

  test('passes the Windows form of the shim entries to node', async () => {
    const { nodePath, stdout } = await runShim()
    assert.equal(stdout, nodePath)
  })

  test('prepends the shim entries to an inherited NODE_PATH with a semicolon', async () => {
    const { nodePath, stdout } = await runShim('C:\\inherited\\node_modules')
    assert.equal(stdout, `${nodePath};C:\\inherited\\node_modules`)
  })
})

describeOnWindows('sh shim running node from PATH without MSYS path conversion', () => {
  // Regression for pnpm/pnpm#12845: Cygwin, unlike MSYS2, starts the native
  // Windows node found on PATH without rewriting POSIX path arguments, so node
  // looked for the target under C:\cygdrive\c\... Git Bash behaves the same way
  // when MSYS2_ARG_CONV_EXCL excludes every argument from conversion.
  test('passes the Windows form of the target to node', async () => {
    const tempDir = temporaryDirectory()
    const target = path.join(tempDir, 'tool.js')
    fs.writeFileSync(target, '#!/usr/bin/env node\nconsole.log("SHIM_OK")\n', 'utf8')
    const shim = path.join(tempDir, 'tool')
    await cmdShim(target, shim, { createCmdFile: false, createPwshFile: false })

    const bash = process.env.PROGRAMFILES
      ? path.join(process.env.PROGRAMFILES, 'Git', 'bin', 'bash.exe')
      : 'bash'
    // Run the shim by its absolute POSIX path, as a PATH lookup does, so the
    // shim's basedir is a POSIX path.
    const r = spawnSync(bash, ['--noprofile', '--norc', '-c', 'exec "$(cygpath -u "$1")"', 'bash', shim], {
      encoding: 'utf8',
      env: { ...process.env, MSYS2_ARG_CONV_EXCL: '*' },
    })

    assert.equal(r.status, 0, `bash exited ${r.status}\nstdout: ${r.stdout}\nstderr: ${r.stderr}`)
    assert.equal(r.stdout.trim(), 'SHIM_OK')
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

  // Where no default path is compiled in, as on Nix, command -p searches the
  // caller's PATH. No test host behaves that way, so the shim's command -p is
  // rewritten to the plain command such a shell amounts to.
  test('skips node_modules and relative PATH entries when command -p searches PATH', async () => {
    const tempDir = temporaryDirectory()
    const binDir = await makeShimmedTool(tempDir)
    const shim = path.join(binDir, 'tsc')
    writeExecutable(shim, fs.readFileSync(shim, 'utf8').replaceAll('command -p ', 'command '))
    const decoyDir = plantHijackTreeAndDecoys(tempDir)
    const callersPath = [path.dirname(process.execPath), process.env.PATH].join(path.delimiter)
    const run = (PATH, cwd) => spawnSync(path.join(binDir, 'tsc-link'), [], {
      cwd,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
      env: { ...process.env, PATH },
    }).stdout.trim()

    assert.equal(
      run([decoyDir, callersPath].join(path.delimiter), tempDir),
      'hijacked',
      'precondition: the rewritten shim resolves its helpers through PATH'
    )
    const nodeModulesBin = path.join(tempDir, 'proj', 'node_modules', '.bin')
    fs.mkdirSync(path.dirname(nodeModulesBin), { recursive: true })
    fs.renameSync(decoyDir, nodeModulesBin)
    assert.equal(
      run([nodeModulesBin, callersPath].join(path.delimiter), tempDir),
      'tsc-output',
      'the shim took a helper from a node_modules entry of PATH'
    )
    assert.equal(
      run(['.bin', callersPath].join(path.delimiter), path.dirname(nodeModulesBin)),
      'tsc-output',
      'the shim took a helper from a relative entry of PATH'
    )
    assert.equal(
      run(['', callersPath].join(path.delimiter), nodeModulesBin),
      'tsc-output',
      'the shim took a helper from an empty entry of PATH'
    )
    // Nothing survives the filter here, and an empty PATH would search the
    // current directory, which holds the decoys.
    fs.mkdirSync(path.join(nodeModulesBin, 'node-dir'))
    fs.symlinkSync(process.execPath, path.join(nodeModulesBin, 'node-dir', 'node'))
    assert.notEqual(
      run('node-dir', nodeModulesBin),
      'hijacked',
      'the shim took a helper from the current directory'
    )
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

describeOnPosix('sh shim picks its Windows path converter', () => {
  // command -p searches the system default path, which a test cannot plant
  // into, and no test host reports itself as Cygwin or WSL2. So the branch is
  // lifted out of a generated shim and run with the uname and the two
  // converters stood in for. What that leaves unproven — that the header
  // reaches them through command -p — the snapshots pin.
  const BASEDIR = '/proj/node_modules/.bin'

  const writeExecutable = (file, body) => {
    fs.writeFileSync(file, body, 'utf8')
    fs.chmodSync(file, 0o755)
  }

  const runPlatformBranch = (shimBody, uname, systemConverter, callersPath) => {
    const caseHead = 'case `command -p uname -a` in'
    const start = shimBody.indexOf(caseHead)
    assert.notEqual(start, -1, 'the header must select a platform')
    const end = shimBody.indexOf('\nesac\n', start) + '\nesac\n'.length
    const branch = shimBody.slice(start, end)
      .replaceAll('`command -p uname -a`', '"$fake_uname"')
      .replaceAll('command -p cygpath', '"$system_converter"')
      .replaceAll('command -p wslpath', '"$system_converter"')
    const script = `basedir=${BASEDIR}\nbasedir_win="$basedir"\nexe=""\nmsys=""\n${branch}\nprintf '%s\\n%s' "$basedir_win" "$exe"`

    const r = spawnSync('/bin/sh', ['-c', script], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
      env: { fake_uname: uname, system_converter: systemConverter, PATH: callersPath },
    })
    assert.equal(r.status, 0, `sh exited ${r.status}\nstderr: ${r.stderr}`)
    return r.stdout.split('\n')
  }

  test('prefers the system converter and still falls back to PATH', async (t) => {
    const tempDir = temporaryDirectory()
    const target = path.join(tempDir, 'tool.js')
    fs.writeFileSync(target, '#!/usr/bin/env node\n', 'utf8')
    const shim = path.join(tempDir, 'tool')
    await cmdShim(target, shim, { createCmdFile: false })
    const shimBody = fs.readFileSync(shim, 'utf8')

    const answering = path.join(tempDir, 'answering')
    writeExecutable(answering, '#!/bin/sh\necho \'/system/win\'\n')
    const silent = path.join(tempDir, 'silent')
    writeExecutable(silent, '#!/bin/sh\n')
    const absent = path.join(tempDir, 'absent')
    const decoys = path.join(tempDir, 'decoy')
    fs.mkdirSync(decoys)
    for (const helper of ['cygpath', 'wslpath']) {
      writeExecutable(path.join(decoys, helper), '#!/bin/sh\necho \'/decoy/win\'\n')
    }

    for (const uname of ['MINGW64_NT-10.0', 'Linux 5.15.0 WSL2']) {
      t.assert.deepEqual(
        runPlatformBranch(shimBody, uname, answering, decoys),
        ['/system/win', '.exe'],
        `${uname}: the system converter must win over the one on PATH`
      )
      t.assert.deepEqual(
        runPlatformBranch(shimBody, uname, silent, decoys),
        ['/decoy/win', '.exe'],
        `${uname}: an empty answer from the system converter must fall back to PATH`
      )
      t.assert.deepEqual(
        runPlatformBranch(shimBody, uname, absent, decoys),
        ['/decoy/win', '.exe'],
        `${uname}: no system converter must fall back to PATH`
      )
    }
    t.assert.deepEqual(
      runPlatformBranch(shimBody, 'MINGW64_NT-10.0', absent, ''),
      [BASEDIR, '.exe'],
      'MSYS with no converter at all must keep the POSIX basedir instead of failing'
    )
    t.assert.deepEqual(
      runPlatformBranch(shimBody, 'Linux 5.15.0 WSL2', absent, ''),
      [BASEDIR, ''],
      'WSL2 with no converter at all must not claim a Windows exe'
    )
  })
})

describeOnPosix('sh shim run through a symlinked package directory', () => {
  // Node normalizes the `..` of its script path lexically, so a target reached
  // from `node_modules/vite/node_modules/.bin` must not climb out of the
  // `node_modules/vite` symlink.
  test('runs the target in the virtual store', async () => {
    const root = fs.realpathSync(temporaryDirectory())
    const virtualStoreDir = path.join(root, 'node_modules/.pnpm/vite@6.0.0/node_modules')
    const binDir = path.join(virtualStoreDir, 'vite/node_modules/.bin')
    const target = path.join(virtualStoreDir, 'esbuild/bin/esbuild')
    fs.mkdirSync(binDir, { recursive: true })
    fs.mkdirSync(path.dirname(target), { recursive: true })
    fs.writeFileSync(target, '#!/usr/bin/env node\nprocess.stdout.write(__filename)\n', 'utf8')
    fs.symlinkSync(path.join(virtualStoreDir, 'vite'), path.join(root, 'node_modules/vite'))
    await cmdShim(target, path.join(binDir, 'esbuild'), { createCmdFile: false })

    const r = spawnSync('node_modules/vite/node_modules/.bin/esbuild', {
      cwd: root,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
    })

    assert.equal(r.status, 0, `shim exited ${r.status}\nstderr: ${r.stderr}`)
    assert.equal(r.stdout, target)
  })
})

describeOnPosix('sh shim written through a symlinked bin directory', () => {
  // The shim climbs from its physical directory, so a bin directory that is a
  // symlink to a deeper directory must not shift the relative target.
  test('runs the target', async () => {
    const root = fs.realpathSync(temporaryDirectory())
    const target = path.join(root, 'pkg/cli.js')
    fs.mkdirSync(path.dirname(target), { recursive: true })
    fs.writeFileSync(target, '#!/usr/bin/env node\nprocess.stdout.write(__filename)\n', 'utf8')
    const physicalBinDir = path.join(root, 'storage/deep/bin')
    fs.mkdirSync(physicalBinDir, { recursive: true })
    fs.symlinkSync(physicalBinDir, path.join(root, 'bin'))
    await cmdShim(target, path.join(root, 'bin/tool'), { createCmdFile: false })

    const r = spawnSync(path.join(root, 'bin/tool'), {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
    })

    assert.equal(r.status, 0, `shim exited ${r.status}\nstderr: ${r.stderr}`)
    assert.equal(r.stdout, target)
  })
})
