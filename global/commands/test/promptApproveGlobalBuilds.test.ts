import { describe, expect, it, jest } from '@jest/globals'
import type { CommandHandlerMap } from '@pnpm/cli.command'
import { promptApproveGlobalBuilds } from '@pnpm/global.commands'

describe('promptApproveGlobalBuilds', () => {
  const baseOpts = {
    globalPkgDir: '/global/pnpm',
    installDir: '/global/pnpm/abc-123',
    ignoredBuilds: new Set(['esbuild']),
    allowBuilds: { esbuild: true },
    inheritedOpts: {},
  }

  it('passes modulesDir as undefined to approve-builds', async () => {
    const wasTty = process.stdin.isTTY
    Object.defineProperty(process.stdin, 'isTTY', { value: true, configurable: true })
    try {
      const approveHandler = jest.fn(async () => {})
      const commands: CommandHandlerMap = {
        'approve-builds': approveHandler as any, // eslint-disable-line @typescript-eslint/no-explicit-any
      }
      await promptApproveGlobalBuilds(baseOpts, commands)
      expect(approveHandler).toHaveBeenCalledTimes(1)
      // Forwarding an absolute modulesDir would later be re-joined with
      // lockfileDir during install, producing a doubled path on Windows
      // (path.join does not collapse an embedded absolute path). Leaving it
      // undefined lets downstream code derive it from lockfileDir.
      const passedOpts = approveHandler.mock.calls[0][0] as Record<string, unknown>
      expect(passedOpts.modulesDir).toBeUndefined()
      expect(passedOpts.lockfileDir).toBe(baseOpts.installDir)
      expect(passedOpts.dir).toBe(baseOpts.installDir)
    } finally {
      Object.defineProperty(process.stdin, 'isTTY', { value: wasTty, configurable: true })
    }
  })

  it('skips the prompt when there are no ignored builds', async () => {
    const approveHandler = jest.fn(async () => {})
    const commands: CommandHandlerMap = {
      'approve-builds': approveHandler as any, // eslint-disable-line @typescript-eslint/no-explicit-any
    }
    await promptApproveGlobalBuilds({ ...baseOpts, ignoredBuilds: undefined }, commands)
    expect(approveHandler).not.toHaveBeenCalled()
  })
})
