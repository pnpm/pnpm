import { execSync } from 'node:child_process'
import crypto from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'

import { temporaryDirectory } from 'tempy'

import { writeBufferToCafs } from '../src/writeBufferToCafs.js'

const testDir = new URL('.', import.meta.url).pathname

describe('writeBufferToCafs', () => {
  it('should write directly to the final CAS path', () => {
    const storeDir = temporaryDirectory()
    const fileDest = 'abc'
    const buffer = Buffer.from('abc')
    const fullFileDest = path.join(storeDir, fileDest)
    const digest = crypto.hash('sha512', buffer, 'hex')
    writeBufferToCafs(new Map(), storeDir, buffer, fileDest, 420, { digest, algorithm: 'sha512' })
    expect(fs.readFileSync(fullFileDest, 'utf8')).toBe('abc')
  })

  it('should handle EEXIST when the existing file has correct integrity', () => {
    const storeDir = temporaryDirectory()
    const fileDest = 'abc'
    const buffer = Buffer.from('abc')
    const fullFileDest = path.join(storeDir, fileDest)
    const digest = crypto.hash('sha512', buffer, 'hex')
    const integrity = { digest, algorithm: 'sha512' }
    const locker = new Map<string, number>()

    // Simulate another process creating the file before our exclusive write
    fs.mkdirSync(path.dirname(fullFileDest), { recursive: true })
    fs.writeFileSync(fullFileDest, buffer)

    // Our call should find the file via stat, verify integrity, and cache it
    const result = writeBufferToCafs(locker, storeDir, buffer, fileDest, 420, integrity)
    expect(result.filePath).toBe(fullFileDest)
    expect(locker.has(fullFileDest)).toBe(true)
    expect(fs.readFileSync(fullFileDest, 'utf8')).toBe('abc')
  })

  it('should overwrite an existing file with wrong integrity without deleting it first', () => {
    const storeDir = temporaryDirectory()
    const fileDest = 'abc'
    const buffer = Buffer.from('abc')
    const fullFileDest = path.join(storeDir, fileDest)
    const digest = crypto.hash('sha512', buffer, 'hex')
    const integrity = { digest, algorithm: 'sha512' }
    const locker = new Map<string, number>()

    // Create a file with wrong content (simulating corruption or partial write)
    fs.mkdirSync(path.dirname(fullFileDest), { recursive: true })
    fs.writeFileSync(fullFileDest, 'wrong content')

    // Should detect mismatch and overwrite (not delete + recreate)
    const result = writeBufferToCafs(locker, storeDir, buffer, fileDest, 420, integrity)
    expect(result.filePath).toBe(fullFileDest)
    expect(locker.has(fullFileDest)).toBe(true)
    expect(fs.readFileSync(fullFileDest, 'utf8')).toBe('abc')
  })

  it('should handle concurrent writes from multiple processes without corruption', () => {
    const storeDir = temporaryDirectory()
    const fileDest = 'abc'
    const content = 'concurrent-test-content'
    const buffer = Buffer.from(content)
    const fullFileDest = path.join(storeDir, fileDest)
    const digest = crypto.hash('sha512', buffer, 'hex')

    // Write a spawner script that launches N child processes in parallel,
    // each calling writeBufferToCafs with identical content, then waits for all.
    // Cross-platform: uses child_process.fork, works on Windows and POSIX.
    const libPath = path.resolve(testDir, '../lib/writeBufferToCafs.js')
    const spawnerScript = path.join(storeDir, '_spawner.mjs')
    fs.writeFileSync(spawnerScript, `
      import { fork } from 'node:child_process';
      const N = 8;
      const workerScript = process.argv[2];
      const procs = [];
      for (let i = 0; i < N; i++) {
        procs.push(new Promise((resolve, reject) => {
          const p = fork(workerScript, [], { stdio: 'pipe' });
          let stderr = '';
          p.stderr.on('data', d => { stderr += d; });
          p.on('exit', code => code === 0 ? resolve() : reject(new Error('Process ' + i + ' failed: ' + stderr)));
        }));
      }
      const results = await Promise.allSettled(procs);
      const failures = results.filter(r => r.status === 'rejected');
      if (failures.length > 0) {
        for (const f of failures) console.error(f.reason.message);
        process.exit(1);
      }
    `)

    const workerScript = path.join(storeDir, '_worker.mjs')
    fs.writeFileSync(workerScript, `
      import { writeBufferToCafs } from ${JSON.stringify(libPath)};
      const buffer = Buffer.from(${JSON.stringify(content)});
      const locker = new Map();
      writeBufferToCafs(locker, ${JSON.stringify(storeDir)}, buffer, ${JSON.stringify(fileDest)}, 420, { digest: ${JSON.stringify(digest)}, algorithm: 'sha512' });
    `)

    execSync(`node ${JSON.stringify(spawnerScript)} ${JSON.stringify(workerScript)}`, {
      timeout: 30000,
      stdio: 'pipe',
    })

    // The file should exist with correct content
    expect(fs.readFileSync(fullFileDest, 'utf8')).toBe(content)

    // Verify integrity of the final file
    const finalDigest = crypto.hash('sha512', fs.readFileSync(fullFileDest), 'hex')
    expect(finalDigest).toBe(digest)
  })

  it('should populate the locker cache when a file already exists with correct integrity', () => {
    const storeDir = temporaryDirectory()
    const fileDest = 'abc'
    const buffer = Buffer.from('abc')
    const digest = crypto.hash('sha512', buffer, 'hex')
    const integrity = { digest, algorithm: 'sha512' }
    const locker = new Map<string, number>()

    // First write creates the file
    writeBufferToCafs(locker, storeDir, buffer, fileDest, 420, integrity)
    // Clear the locker to simulate a fresh lookup
    locker.clear()

    // Second call should find the file on disk and cache it
    const result = writeBufferToCafs(locker, storeDir, buffer, fileDest, 420, integrity)
    const fullFileDest = path.join(storeDir, fileDest)
    expect(locker.get(fullFileDest)).toBe(result.checkedAt)

    // Third call should return from locker cache without hitting disk
    const cached = writeBufferToCafs(locker, storeDir, buffer, fileDest, 420, integrity)
    expect(cached.checkedAt).toBe(result.checkedAt)
  })
})
