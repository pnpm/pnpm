#!/usr/bin/env node
// Fails when a Windows binary imports the Visual C++ runtime. A clean Windows
// install does not ship VCRUNTIME140.dll, so such a binary cannot start there
// (https://github.com/pnpm/pnpm/issues/15723).
import { execFileSync } from 'node:child_process'
import path from 'node:path'

const CRT_DLL = /^(?:vcruntime\d|msvcp\d|ucrtbase|api-ms-win-crt-)/i

const binaries = process.argv.slice(2)
if (binaries.length === 0) {
  console.error('Usage: check-static-crt.mjs <binary>...')
  process.exit(2)
}

const dumpbin = findDumpbin()
let failed = false
for (const binary of binaries) {
  const dependencies = listDependencies(dumpbin, binary)
  console.log(`${binary}: ${dependencies.join(', ')}`)
  // Every Windows image imports KERNEL32; an empty list means the output
  // format changed and the check would pass vacuously.
  if (!dependencies.some((dll) => /^kernel32\.dll$/i.test(dll))) {
    console.error(`::error::found no KERNEL32.dll import in ${binary}; cannot read its dependencies`)
    failed = true
    continue
  }
  const crt = dependencies.filter((dll) => CRT_DLL.test(dll))
  if (crt.length > 0) {
    console.error(`::error::${binary} links the C runtime dynamically: ${crt.join(', ')}`)
    failed = true
  }
}
process.exitCode = failed ? 1 : 0

function findDumpbin () {
  const vswhere = path.join(process.env['ProgramFiles(x86)'], 'Microsoft Visual Studio', 'Installer', 'vswhere.exe')
  const found = execFileSync(vswhere, [
    '-latest',
    '-products', '*',
    '-requires', 'Microsoft.VisualStudio.Component.VC.Tools.x86.x64',
    '-find', 'VC/Tools/MSVC/**/bin/Hostx64/x64/dumpbin.exe',
  ], { encoding: 'utf8' }).split(/\r?\n/).filter(Boolean)
  if (found.length === 0) throw new Error('vswhere found no dumpbin.exe')
  return found[0]
}

function listDependencies (dumpbin, binary) {
  const output = execFileSync(dumpbin, ['/nologo', '/dependents', binary], { encoding: 'utf8' })
  return output.split(/\r?\n/).map((line) => line.trim()).filter((line) => /^[\w.-]+\.dll$/i.test(line))
}
