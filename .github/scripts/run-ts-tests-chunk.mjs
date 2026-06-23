#!/usr/bin/env node
import { spawn, spawnSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { mkdir, readdir, stat, writeFile } from 'node:fs/promises'
import path from 'node:path'
import { performance } from 'node:perf_hooks'

const rootDir = process.cwd()
const pnpmCommand = resolveCommand('pn')
const WINDOWS_SHELL_COMMAND_LENGTH_LIMIT = 7000
const DEFAULT_COMMAND_LENGTH_LIMIT = 100000
const { chunk, chunks, dryRun, script, summary } = parseArgs(process.argv.slice(2))

if (!dryRun) {
  await runPnpm(['run', 'prepare-fixtures'])
  await runPnpm(['run', 'remove-temp-dir'])
}

const packages = await listSelectedPackages(script)
const tasks = await listTestTasks(packages)
const selectedTasks = selectChunk(tasks, { chunk, chunks })
const selectedPackages = groupJestTasksByPackage(selectedTasks)
const executionStatus = {}

console.log(`Selected ${selectedTasks.length} of ${tasks.length} test tasks for chunk ${chunk}/${chunks}`)

if (!dryRun) {
  for (const pkg of packages) {
    const selectedPackage = selectedPackages.get(pkg.path)
    if (selectedPackage != null) {
      await runJestPackage(pkg, selectedPackage)
    } else if (selectedTasks.some((task) => task.kind === 'script' && task.packagePath === pkg.path)) {
      await runScriptTask(pkg)
    }
  }
  await writeSummary(summary, executionStatus)
}

const failures = Object.values(executionStatus).filter((status) => status.status === 'failure')
if (failures.length > 0) {
  process.exitCode = 1
}

async function listSelectedPackages (scriptName) {
  const filterArgs = scriptName === 'ci:test-branch' ? ['--filter=...[origin/main]'] : []
  const stdout = await capturePnpm([...filterArgs, '-r', 'list', '--depth', '-1', '--json'])
  return JSON.parse(stdout)
    .map((pkg) => ({
      path: pkg.path,
      manifest: readManifest(pkg.path),
    }))
    .filter((pkg) => pkg.manifest.scripts?.['.test'] != null)
}

async function listTestTasks (packages) {
  const tasks = []
  for (const pkg of packages) {
    if (!usesJest(pkg.manifest.scripts)) {
      tasks.push({
        id: normalizePath(path.relative(rootDir, pkg.path)),
        kind: 'script',
        packagePath: pkg.path,
        weight: 1,
      })
      continue
    }

    const testFiles = await findJestTestFiles(pkg.path)
    tasks.push(...await Promise.all(testFiles.map(async (file) => {
      const fileStat = await stat(file)
      return {
        file,
        id: normalizePath(path.relative(rootDir, file)),
        kind: 'jest',
        packagePath: pkg.path,
        weight: Math.max(1, fileStat.size),
      }
    })))
  }
  return tasks
}

function usesJest (scripts) {
  return Object.entries(scripts).some(([name, script]) => (
    (name === '.test' || name.startsWith('.test:')) && script.includes('jest')
  ))
}

function readRegistryMockPort (scripts) {
  for (const script of Object.values(scripts ?? {})) {
    const match = /PNPM_REGISTRY_MOCK_PORT=(\d+)/.exec(script)
    if (match != null) return match[1]
  }
  return undefined
}

function selectChunk (tasks, opts) {
  const groups = Array.from({ length: opts.chunks }, () => ({ tasks: [], weight: 0 }))
  for (const task of [...tasks].sort(compareTasksByWeight)) {
    const group = groups.reduce((best, candidate) => (
      candidate.weight < best.weight ? candidate : best
    ))
    group.tasks.push(task)
    group.weight += task.weight
  }
  return groups[opts.chunk - 1].tasks.sort((a, b) => a.id.localeCompare(b.id))
}

function groupJestTasksByPackage (tasks) {
  const selectedPackages = new Map()
  for (const task of tasks) {
    if (task.kind !== 'jest') continue
    const selectedPackage = selectedPackages.get(task.packagePath) ?? { files: [] }
    selectedPackage.files.push(task.file)
    selectedPackages.set(task.packagePath, selectedPackage)
  }
  for (const selectedPackage of selectedPackages.values()) {
    selectedPackage.files.sort()
  }
  return selectedPackages
}

async function runJestPackage (pkg, selectedPackage) {
  const startedAt = performance.now()
  const relDir = normalizePath(path.relative(rootDir, pkg.path))
  const relFiles = selectedPackage.files.map((file) => normalizePath(path.relative(pkg.path, file)))
  const batches = getJestFileBatches(pkg, relFiles)
  console.log(`Running ${relFiles.length} Jest file(s) in ${relDir}`)

  const env = {
    ...process.env,
    NODE_OPTIONS: withJestNodeOptions(process.env.NODE_OPTIONS),
    PNPM_SCRIPT_SRC_DIR: pkg.path,
  }
  prependPath(env, [
    path.join(pkg.path, 'node_modules', '.bin'),
    path.join(rootDir, 'node_modules', '.bin'),
  ])
  // Some packages pin PNPM_REGISTRY_MOCK_PORT in their `.test` script (e.g. a
  // fixture whose dependency is a tarball URL baked to that port). Running jest
  // directly bypasses the script, so carry the pin over from the manifest.
  const registryMockPort = readRegistryMockPort(pkg.manifest.scripts)
  if (registryMockPort != null) {
    env.PNPM_REGISTRY_MOCK_PORT = registryMockPort
  }

  let status = 'passed'
  let exitCode = 0
  try {
    if (pkg.manifest.scripts.pretest != null) {
      await runPnpm(['--dir', pkg.path, 'run', 'pretest'], { env })
    }
    for (const [index, files] of batches.entries()) {
      if (batches.length > 1) {
        console.log(`Running batch ${index + 1}/${batches.length} (${files.length} Jest file(s)) in ${relDir}`)
      }
      await runPnpm(['--dir', pkg.path, 'exec', 'jest', '--runTestsByPath', ...files], { env })
    }
  } catch (err) {
    status = 'failure'
    exitCode = err.exitCode ?? 1
  }

  executionStatus[pkg.path] = {
    status,
    duration: performance.now() - startedAt,
    exitCode,
  }
}

async function runScriptTask (pkg) {
  const startedAt = performance.now()
  const relDir = normalizePath(path.relative(rootDir, pkg.path))
  console.log(`Running non-Jest test script in ${relDir}`)

  let status = 'passed'
  let exitCode = 0
  try {
    if (pkg.manifest.name === 'pd') {
      await runCommand('node', ['pd.js', '--version'], { cwd: pkg.path })
    } else {
      throw new Error(`Unsupported non-Jest .test script in ${relDir}: ${pkg.manifest.scripts['.test']}`)
    }
  } catch (err) {
    status = 'failure'
    exitCode = err.exitCode ?? 1
  }

  executionStatus[pkg.path] = {
    status,
    duration: performance.now() - startedAt,
    exitCode,
  }
}

async function findJestTestFiles (packageDir) {
  const files = []
  await collectTestFiles(path.join(packageDir, 'test'), files, isTestDirFile)
  await collectTestFiles(path.join(packageDir, 'src'), files, isSrcTestFile)
  return files.sort()
}

async function collectTestFiles (dir, files, matches) {
  let entries
  try {
    entries = await readdir(dir, { withFileTypes: true })
  } catch (err) {
    if (err.code === 'ENOENT') return
    throw err
  }

  for (const entry of entries) {
    const entryPath = path.join(dir, entry.name)
    if (entry.isDirectory()) {
      if (entry.name === 'fixtures' || entry.name === '__fixtures__') continue
      if (isTestUtilsDir(entryPath)) continue
      await collectTestFiles(entryPath, files, matches)
    } else if (entry.isFile() && matches(entryPath)) {
      files.push(entryPath)
    }
  }
}

function isTestDirFile (file) {
  return /\.[jt]sx?$/.test(file) && !file.endsWith('.d.ts')
}

function isSrcTestFile (file) {
  return file.endsWith('.test.ts')
}

function isTestUtilsDir (dir) {
  const segments = normalizePath(path.relative(rootDir, dir)).split('/')
  const testIndex = segments.indexOf('test')
  return testIndex !== -1 && segments.slice(testIndex + 1).includes('utils')
}

async function writeSummary (summaryPath, status) {
  await mkdir(path.dirname(summaryPath), { recursive: true })
  await writeFile(summaryPath, JSON.stringify({ executionStatus: status }, null, 2) + '\n')
}

function readManifest (packageDir) {
  return JSON.parse(readFileSyncUtf8(path.join(packageDir, 'package.json')))
}

function readFileSyncUtf8 (file) {
  return readFileSync(file, 'utf8')
}

function compareTasksByWeight (a, b) {
  return b.weight - a.weight || a.id.localeCompare(b.id)
}

function withJestNodeOptions (current = '') {
  const options = current.split(/\s+/).filter(Boolean)
  addNodeOption(options, '--experimental-vm-modules')
  addNodeOption(options, '--disable-warning=ExperimentalWarning')
  addNodeOption(options, '--disable-warning=DEP0169')
  if (!options.some((option) => option.startsWith('--max-old-space-size='))) {
    options.push('--max-old-space-size=6144')
  }
  return options.join(' ')
}

function addNodeOption (options, option) {
  if (!options.includes(option)) {
    options.push(option)
  }
}

function getJestFileBatches (pkg, relFiles) {
  const isolatedFiles = Array.from(getExplicitJestFiles(pkg.manifest.scripts))
    .filter((file) => relFiles.includes(file))
  if (isolatedFiles.length === 0 && !hasSplitJestScripts(pkg.manifest.scripts)) return [relFiles]

  const isolatedFileSet = new Set(isolatedFiles)
  const remainingFiles = relFiles.filter((file) => !isolatedFileSet.has(file))
  return [
    ...isolatedFiles.map((file) => [file]),
    ...splitJestFilesByCommandLength(pkg, remainingFiles),
  ].filter((files) => files.length > 0)
}

function hasSplitJestScripts (scripts) {
  return Object.entries(scripts).some(([name, script]) => name.startsWith('.test:') && script.includes('jest'))
}

function getExplicitJestFiles (scripts) {
  const files = new Set()
  for (const [name, script] of Object.entries(scripts)) {
    if (!name.startsWith('.test:') || !script.includes('jest')) continue
    for (const match of script.matchAll(/(?:^|\s)(test\/[^\s"'`]+?\.[jt]sx?)(?=\s|$)/g)) {
      files.add(match[1])
    }
  }
  return files
}

function splitJestFilesByCommandLength (pkg, files) {
  const limit = process.platform === 'win32' ? WINDOWS_SHELL_COMMAND_LENGTH_LIMIT : DEFAULT_COMMAND_LENGTH_LIMIT
  const baseArgs = [pnpmCommand, '--dir', pkg.path, 'exec', 'jest', '--runTestsByPath']
  const chunks = []
  let currentChunk = []
  let currentLength = estimateCommandLength(baseArgs)

  for (const file of files) {
    const fileLength = estimateCommandLength([file])
    if (currentChunk.length > 0 && currentLength + fileLength > limit) {
      chunks.push(currentChunk)
      currentChunk = []
      currentLength = estimateCommandLength(baseArgs)
    }
    currentChunk.push(file)
    currentLength += fileLength
  }

  if (currentChunk.length > 0) {
    chunks.push(currentChunk)
  }
  return chunks
}

function estimateCommandLength (args) {
  return args.reduce((total, arg) => total + arg.length + 3, 0)
}

function prependPath (env, dirs) {
  const pathKey = getPathKey(env)
  env[pathKey] = [
    ...dirs,
    env[pathKey],
  ].filter(Boolean).join(path.delimiter)
}

function getPathKey (env) {
  return Object.keys(env).find((key) => key.toLowerCase() === 'path') ?? 'PATH'
}

function runPnpm (args, opts = {}) {
  return runCommand(pnpmCommand, args, opts)
}

function capturePnpm (args) {
  return captureCommand(pnpmCommand, args)
}

function runCommand (command, args, opts = {}) {
  return new Promise((resolve, reject) => {
    const spawnTarget = resolveSpawnTarget(command, args)
    const child = spawn(spawnTarget.command, spawnTarget.args, {
      cwd: opts.cwd ?? rootDir,
      env: opts.env ?? process.env,
      shell: spawnTarget.shell,
      stdio: 'inherit',
    })
    child.on('error', reject)
    child.on('close', (code, signal) => {
      if (signal) {
        reject(Object.assign(new Error(`${command} terminated by signal ${signal}`), { exitCode: 1 }))
      } else if (code === 0) {
        resolve()
      } else {
        reject(Object.assign(new Error(`${command} exited with code ${code}`), { exitCode: code ?? 1 }))
      }
    })
  })
}

function captureCommand (command, args) {
  return new Promise((resolve, reject) => {
    let stdout = ''
    const spawnTarget = resolveSpawnTarget(command, args)
    const child = spawn(spawnTarget.command, spawnTarget.args, {
      cwd: rootDir,
      shell: spawnTarget.shell,
      stdio: ['ignore', 'pipe', 'inherit'],
    })
    child.stdout.setEncoding('utf8')
    child.stdout.on('data', (chunk) => {
      stdout += chunk
    })
    child.on('error', reject)
    child.on('close', (code, signal) => {
      if (signal) {
        reject(Object.assign(new Error(`${command} terminated by signal ${signal}`), { exitCode: 1 }))
      } else if (code === 0) {
        resolve(stdout)
      } else {
        reject(Object.assign(new Error(`${command} exited with code ${code}`), { exitCode: code ?? 1 }))
      }
    })
  })
}

function resolveCommand (command) {
  const result = process.platform === 'win32'
    ? spawnSync('where', [command], { encoding: 'utf8' })
    : spawnSync('sh', ['-c', `command -v ${command}`], { encoding: 'utf8' })
  if (result.error != null) {
    throw result.error
  }
  if (result.status !== 0) {
    throw new Error(`Cannot resolve command: ${command}`)
  }
  return result.stdout.split(/\r?\n/).find(Boolean) ?? command
}

// On Windows the command (e.g. `pn`) resolves to a `.cmd` shim, which Node
// refuses to spawn without a shell (CVE-2024-27980). But `spawn(cmd, args,
// { shell: true })` joins the args into one string without quoting, so an arg
// containing a space would be split into separate tokens. Build the quoted
// command line ourselves and hand it to cmd.exe as a single string — the same
// thing Node does internally for the non-shell Windows path.
function resolveSpawnTarget (command, args) {
  if (process.platform !== 'win32') {
    return { command, args, shell: false }
  }
  validateWindowsShellArgs([command, ...args])
  return {
    command: [command, ...args].map(quoteWindowsArg).join(' '),
    args: undefined,
    shell: true,
  }
}

function quoteWindowsArg (arg) {
  return arg === '' || /\s/.test(arg) ? `"${arg}"` : arg
}

function validateWindowsShellArgs (args) {
  for (const arg of args) {
    // `"` is rejected alongside the shell metacharacters so quoteWindowsArg can
    // wrap spaced args in double quotes without needing to escape inner quotes.
    if (/[&|<>^%"\r\n]/.test(arg)) {
      throw new Error(`Cannot run command with Windows shell metacharacters: ${arg}`)
    }
  }
}

function normalizePath (file) {
  return file.split(path.sep).join('/')
}

function parseArgs (args) {
  let chunk
  let chunks
  let script
  let summary = 'pnpm-exec-summary.json'
  let dryRun = false

  for (let i = 0; i < args.length; i++) {
    const arg = args[i]
    if (arg === '--chunk') {
      chunk = Number(args[++i])
    } else if (arg === '--chunks') {
      chunks = Number(args[++i])
    } else if (arg === '--dry-run') {
      dryRun = true
    } else if (arg === '--script') {
      script = args[++i]
    } else if (arg === '--summary') {
      summary = args[++i]
    } else {
      usage(`unknown argument: ${arg}`)
    }
  }

  if (script !== 'ci:test-all' && script !== 'ci:test-branch') {
    usage('missing or unsupported --script')
  }
  if (!Number.isInteger(chunk) || chunk < 1) usage('missing or invalid --chunk')
  if (!Number.isInteger(chunks) || chunks < 1) usage('missing or invalid --chunks')
  if (chunk > chunks) usage('--chunk cannot be greater than --chunks')

  return { chunk, chunks, dryRun, script, summary }
}

function usage (message) {
  console.error(message)
  console.error('Usage: run-ts-tests-chunk.mjs --script <ci:test-all|ci:test-branch> --chunk <n> --chunks <n> [--summary <file>] [--dry-run]')
  process.exit(1)
}
