import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'
import { dynamicVcRuntimeDlls, importedDlls } from './assert-windows-crt-static.mjs'

const script = fileURLToPath(new URL('./assert-windows-crt-static.mjs', import.meta.url))

test('a Windows binary may import the universal CRT and kernel32', () => {
  const dlls = ['KERNEL32.dll', 'api-ms-win-crt-runtime-l1-1-0.dll', 'ucrtbase.dll']
  const bytes = peWithImports(dlls)
  assert.deepEqual(importedDlls(bytes), dlls)
  assert.deepEqual(dynamicVcRuntimeDlls(importedDlls(bytes)), [])
  assert.equal(run(bytes), 0)
})

test('VCRUNTIME140.dll is rejected, including the versioned and uppercase names', () => {
  for (const name of ['VCRUNTIME140.dll', 'VCRUNTIME140_1.dll', 'vcruntime140D.dll']) {
    const bytes = peWithImports(['KERNEL32.dll', name], { pe32Plus: name !== 'vcruntime140D.dll' })
    assert.deepEqual(dynamicVcRuntimeDlls(importedDlls(bytes)), [name])
    assert.equal(run(bytes), 1)
  }
})

test('a file that is not a PE fails the check', () => {
  assert.throws(() => importedDlls(Buffer.from('not a pe')), /not a PE file/)
  assert.equal(run(Buffer.from('MZ')), 1)
})

test('import names match objdump', () => {
  const dlls = ['KERNEL32.dll', 'VCRUNTIME140.dll']
  const bytes = peWithImports(dlls)
  const dir = mkdtempSync(path.join(tmpdir(), 'pnpm-crt-'))
  const file = path.join(dir, 'sample.exe')
  writeFileSync(file, bytes)
  const dump = execFileSync('objdump', ['-p', file], { encoding: 'utf8' })
  const fromDump = [...dump.matchAll(/DLL Name:\s*(\S+)/g)].map(match => match[1])
  assert.deepEqual(importedDlls(readFileSync(file)), fromDump)
  assert.deepEqual(fromDump, dlls)
})

function run (bytes) {
  const dir = mkdtempSync(path.join(tmpdir(), 'pnpm-crt-'))
  const file = path.join(dir, 'sample.exe')
  writeFileSync(file, bytes)
  try {
    execFileSync(process.execPath, [script, file], { stdio: 'pipe' })
    return 0
  } catch (error) {
    return error.status ?? 1
  }
}

// Minimal PE whose import directory objdump can read. Only the fields the
// loader and objdump need for DLL names are filled in.
function peWithImports (dllNames, { pe32Plus = true } = {}) {
  const fileAlignment = 0x200
  const sectionAlignment = 0x1000
  const peOffset = 0x80
  const optionalSize = pe32Plus ? 240 : 224
  const headersSize = peOffset + 24 + optionalSize + 40
  const alignedHeaders = Math.ceil(headersSize / fileAlignment) * fileAlignment

  const descriptorsSize = (dllNames.length + 1) * 20
  const nameBlobs = dllNames.map(name => Buffer.from(`${name}\0`, 'ascii'))
  let nameCursor = descriptorsSize
  const nameOffsets = nameBlobs.map(blob => {
    const offset = nameCursor
    nameCursor += blob.length
    return offset
  })
  const section = Buffer.alloc(nameCursor)
  for (const [index, blob] of nameBlobs.entries()) {
    section.writeUInt32LE(sectionAlignment + nameOffsets[index], index * 20 + 12)
    section.writeUInt32LE(1, index * 20 + 16)
    blob.copy(section, nameOffsets[index])
  }

  const rawSectionSize = Math.ceil(section.length / fileAlignment) * fileAlignment
  const file = Buffer.alloc(alignedHeaders + rawSectionSize)
  file.write('MZ', 0, 'ascii')
  file.writeUInt32LE(peOffset, 0x3c)
  file.write('PE\0\0', peOffset, 'ascii')
  const coff = peOffset + 4
  file.writeUInt16LE(pe32Plus ? 0x8664 : 0x14c, coff)
  file.writeUInt16LE(1, coff + 2)
  file.writeUInt16LE(optionalSize, coff + 16)
  file.writeUInt16LE(0x22, coff + 18)
  const optional = coff + 20
  file.writeUInt16LE(pe32Plus ? 0x20b : 0x10b, optional)
  file.writeUInt32LE(sectionAlignment, optional + 32)
  file.writeUInt32LE(fileAlignment, optional + 36)
  file.writeUInt16LE(6, optional + 48)
  const sizeOfImage = sectionAlignment + Math.ceil(section.length / sectionAlignment) * sectionAlignment
  file.writeUInt32LE(sizeOfImage, optional + 56)
  file.writeUInt32LE(alignedHeaders, optional + 60)
  const dataDirectory = optional + (pe32Plus ? 112 : 96)
  file.writeUInt32LE(16, dataDirectory - 4)
  file.writeUInt32LE(sectionAlignment, dataDirectory + 8)
  file.writeUInt32LE(descriptorsSize, dataDirectory + 12)

  const sectionHeader = optional + optionalSize
  file.write('.idata', sectionHeader, 'ascii')
  file.writeUInt32LE(section.length, sectionHeader + 8)
  file.writeUInt32LE(sectionAlignment, sectionHeader + 12)
  file.writeUInt32LE(rawSectionSize, sectionHeader + 16)
  file.writeUInt32LE(alignedHeaders, sectionHeader + 20)
  section.copy(file, alignedHeaders)
  return file
}
