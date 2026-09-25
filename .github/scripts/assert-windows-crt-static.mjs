// Release gate for the windows-msvc CLI: the shipped pnpm.exe must not import
// the Visual C++ runtime. A clean Windows install does not have that DLL.
import { readFileSync } from 'node:fs'
import { pathToFileURL } from 'node:url'

const VC_RUNTIME_DLL = /^vcruntime140/i

export function importedDlls (bytes) {
  const view = Buffer.isBuffer(bytes) ? bytes : Buffer.from(bytes)
  if (view.length < 0x40 || view.toString('ascii', 0, 2) !== 'MZ') {
    throw new Error('not a PE file (missing MZ header)')
  }
  const peOffset = view.readUInt32LE(0x3c)
  if (peOffset + 24 > view.length || view.toString('ascii', peOffset, peOffset + 4) !== 'PE\0\0') {
    throw new Error('not a PE file (missing PE signature)')
  }
  const coff = peOffset + 4
  const sectionCount = view.readUInt16LE(coff + 2)
  const optionalSize = view.readUInt16LE(coff + 16)
  const optional = coff + 20
  if (optional + optionalSize > view.length) {
    throw new Error('truncated PE optional header')
  }
  const magic = view.readUInt16LE(optional)
  let dataDirectoryOffset
  if (magic === 0x10b) dataDirectoryOffset = optional + 96
  else if (magic === 0x20b) dataDirectoryOffset = optional + 112
  else throw new Error(`unsupported PE magic 0x${magic.toString(16)}`)
  if (dataDirectoryOffset > view.length) {
    throw new Error('truncated PE data directories')
  }
  const numberOfRva = view.readUInt32LE(dataDirectoryOffset - 4)
  if (numberOfRva < 2) return []
  const importRva = view.readUInt32LE(dataDirectoryOffset + 8)
  const importSize = view.readUInt32LE(dataDirectoryOffset + 12)
  if (importRva === 0 || importSize === 0) return []

  const sections = []
  let sectionHeader = optional + optionalSize
  for (let index = 0; index < sectionCount; index++) {
    if (sectionHeader + 40 > view.length) throw new Error('truncated section header')
    sections.push({
      virtualAddress: view.readUInt32LE(sectionHeader + 12),
      virtualSize: view.readUInt32LE(sectionHeader + 8),
      rawSize: view.readUInt32LE(sectionHeader + 16),
      rawPointer: view.readUInt32LE(sectionHeader + 20),
    })
    sectionHeader += 40
  }

  const names = []
  let entry = rvaToOffset(view, sections, importRva)
  for (;;) {
    if (entry + 20 > view.length) throw new Error('truncated import directory')
    const originalFirstThunk = view.readUInt32LE(entry)
    const timeDateStamp = view.readUInt32LE(entry + 4)
    const forwarderChain = view.readUInt32LE(entry + 8)
    const nameRva = view.readUInt32LE(entry + 12)
    const firstThunk = view.readUInt32LE(entry + 16)
    if (nameRva === 0 && originalFirstThunk === 0 && timeDateStamp === 0 && forwarderChain === 0 && firstThunk === 0) break
    const nameOffset = rvaToOffset(view, sections, nameRva)
    const end = view.indexOf(0, nameOffset)
    if (end === -1) throw new Error('unterminated import DLL name')
    names.push(view.toString('ascii', nameOffset, end))
    entry += 20
  }
  return names
}

export function dynamicVcRuntimeDlls (dlls) {
  return dlls.filter(name => VC_RUNTIME_DLL.test(name))
}

function rvaToOffset (view, sections, rva) {
  for (const section of sections) {
    const span = Math.max(section.virtualSize, section.rawSize)
    if (rva < section.virtualAddress || rva >= section.virtualAddress + span) continue
    const offset = section.rawPointer + (rva - section.virtualAddress)
    if (offset < 0 || offset >= view.length) throw new Error(`RVA 0x${rva.toString(16)} points outside the file`)
    return offset
  }
  throw new Error(`RVA 0x${rva.toString(16)} is not in any section`)
}

function main () {
  const files = process.argv.slice(2)
  if (files.length === 0) {
    console.error('usage: node assert-windows-crt-static.mjs <pe-file>...')
    process.exit(2)
  }
  let failed = false
  for (const file of files) {
    let linked
    try {
      linked = dynamicVcRuntimeDlls(importedDlls(readFileSync(file)))
    } catch (error) {
      console.error(`${file}: ${error.message}`)
      failed = true
      continue
    }
    if (linked.length > 0) {
      console.error(`${file} dynamically links ${linked.join(', ')}`)
      failed = true
    }
  }
  if (failed) process.exit(1)
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main()
}
