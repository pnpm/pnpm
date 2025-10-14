import fs from 'fs'
import os from 'os'
import path from 'path'
import { describe, it, expect } from '@jest/globals'
import { installPnpmToTools, type SelfUpdateCommandOptions } from '../src/installPnpmToTools.js'

describe('installPnpmToTools', () => {
  it('should install pnpm to tools directory', async () => {
    const pnpmHomeDir = path.join(os.tmpdir(), 'pnpm-test-' + Date.now())

    try {
      const result = await installPnpmToTools('9.1.0', { pnpmHomeDir } as SelfUpdateCommandOptions)

      expect(result.alreadyExisted).toBe(false)
      expect(result.binDir).toContain('pnpm/9.1.0')
      expect(fs.existsSync(result.binDir)).toBe(true)

      // Verify that the pnpm binary exists
      const pnpmBin = path.join(result.binDir, 'pnpm')
      expect(fs.existsSync(pnpmBin)).toBe(true)
    } finally {
      if (fs.existsSync(pnpmHomeDir)) {
        fs.rmSync(pnpmHomeDir, { recursive: true, force: true })
      }
    }
  }, 30000)

  it('should return alreadyExisted=true if pnpm version already installed', async () => {
    const pnpmHomeDir = path.join(os.tmpdir(), 'pnpm-test-' + Date.now())

    try {
      await installPnpmToTools('9.1.0', { pnpmHomeDir } as SelfUpdateCommandOptions)
      const result = await installPnpmToTools('9.1.0', { pnpmHomeDir } as SelfUpdateCommandOptions)

      expect(result.alreadyExisted).toBe(true)
      expect(result.binDir).toContain('pnpm/9.1.0')
    } finally {
      if (fs.existsSync(pnpmHomeDir)) {
        fs.rmSync(pnpmHomeDir, { recursive: true, force: true })
      }
    }
  }, 30000)
})
