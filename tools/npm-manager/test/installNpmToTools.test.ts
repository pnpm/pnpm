import fs from 'fs'
import os from 'os'
import path from 'path'
import { describe, it, expect } from '@jest/globals'
import { installNpmToTools } from '../src/installNpmToTools.js'

describe('installNpmToTools', () => {
  it('should install npm to tools directory', async () => {
    const pnpmHomeDir = path.join(os.tmpdir(), 'pnpm-test-' + Date.now())

    try {
      const result = await installNpmToTools('9.0.0', { pnpmHomeDir })

      expect(result.alreadyExisted).toBe(false)
      expect(result.binDir).toContain(path.join('npm', '9.0.0'))
      expect(fs.existsSync(result.binDir)).toBe(true)
    } finally {
      if (fs.existsSync(pnpmHomeDir)) {
        fs.rmSync(pnpmHomeDir, { recursive: true, force: true })
      }
    }
  }, 30000)

  it('should return alreadyExisted=true if npm version already installed', async () => {
    const pnpmHomeDir = path.join(os.tmpdir(), 'pnpm-test-' + Date.now())

    try {
      await installNpmToTools('9.0.0', { pnpmHomeDir })
      const result = await installNpmToTools('9.0.0', { pnpmHomeDir })

      expect(result.alreadyExisted).toBe(true)
    } finally {
      if (fs.existsSync(pnpmHomeDir)) {
        fs.rmSync(pnpmHomeDir, { recursive: true, force: true })
      }
    }
  }, 30000)
})
