#!/usr/bin/env node
import { spawn } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { mkdir, readdir, stat, writeFile } from 'node:fs/promises'
import path from 'node:path'
import { performance } from 'node:perf_hooks'

const rootDir = process.cwd()
const JEST_FILES_PER_SPLIT_SCRIPT_BATCH = 10
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

    for (const file of await findJestTestFiles(pkg.path)) {
      const fileStat = await stat(file)
      tasks.push({
        file,
        id: normalizePath(path.relative(rootDir, file)),
        kind: 'jest',
        packagePath: pkg.path,
        weight: Math.max(1, fileStat.size),
      })
    }
  }
  return tasks
}

function usesJest (scripts) {
  return Object.entries(scripts).some(([name, script]) => (
    (name === '.test' || name.startsWith('.test:')) && script.includes('jest')
  ))
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
  if (pkg.manifest.name === '@pnpm/installing.deps-installer') {
    env.PNPM_REGISTRY_MOCK_PORT = '7769'
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
  const options = new Set(current.split(/\s+/).filter(Boolean))
  options.add('--experimental-vm-modules')
  options.add('--disable-warning=ExperimentalWarning')
  options.add('--disable-warning=DEP0169')
  return Array.from(options).join(' ')
}

function getJestFileBatches (pkg, relFiles) {
  const isolatedFiles = Array.from(getExplicitJestFiles(pkg.manifest.scripts))
    .filter((file) => relFiles.includes(file))
  if (isolatedFiles.length === 0 && !hasSplitJestScripts(pkg.manifest.scripts)) return [relFiles]

  const isolatedFileSet = new Set(isolatedFiles)
  const remainingFiles = relFiles.filter((file) => !isolatedFileSet.has(file))
  return [
    ...isolatedFiles.map((file) => [file]),
    ...chunkArray(remainingFiles, JEST_FILES_PER_SPLIT_SCRIPT_BATCH),
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

function chunkArray (items, size) {
  const chunks = []
  for (let index = 0; index < items.length; index += size) {
    chunks.push(items.slice(index, index + size))
  }
  return chunks
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
  return runCommand('pn', args, opts)
}

function capturePnpm (args) {
  return captureCommand('pn', args)
}

function runCommand (command, args, opts = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, {
      cwd: opts.cwd ?? rootDir,
      env: opts.env ?? process.env,
      shell: process.platform === 'win32',
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
    const child = spawn(command, args, {
      cwd: rootDir,
      shell: process.platform === 'win32',
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
