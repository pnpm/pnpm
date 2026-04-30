import { describe, expect, it, jest } from '@jest/globals'
import type { CommandHandlerMap } from '@pnpm/cli.command'
import { promptApproveGlobalBuilds, type PromptApproveGlobalBuildsOptions } from '@pnpm/global.commands'
import type { DepPath } from '@pnpm/types'

describe('promptApproveGlobalBuilds', () => {
  const baseOpts: PromptApproveGlobalBuildsOptions = {
    globalPkgDir: '/global/pnpm',
    installDir: '/global/pnpm/abc-123',
    ignoredBuilds: new Set(['esbuild' as DepPath]),
    allowBuilds: { esbuild: true },
    inheritedOpts: {},
  }

  function makeCommands (): { commands: CommandHandlerMap, calls: Array<Record<string, unknown>> } {
    const calls: Array<Record<string, unknown>> = []
    const approveHandler = jest.fn((opts: Record<string, unknown>): Promise<void> => {
      calls.push(opts)
      return Promise.resolve()
    })
    const commands = {
      'approve-builds': approveHandler,
    } as unknown as CommandHandlerMap
    return { commands, calls }
  }

  it('passes modulesDir as undefined to approve-builds', async () => {
    const wasTty = process.stdin.isTTY
    Object.defineProperty(process.stdin, 'isTTY', { value: true, configurable: true })
    try {
      const { commands, calls } = makeCommands()
      await promptApproveGlobalBuilds(baseOpts, commands)
      expect(calls).toHaveLength(1)
      // Forwarding an absolute modulesDir would later be re-joined with
      // lockfileDir during install, producing a doubled path on Windows
      // (path.join does not collapse an embedded absolute path). Leaving it
      // undefined lets downstream code derive it from lockfileDir.
      const passedOpts = calls[0]
      expect(passedOpts.modulesDir).toBeUndefined()
      expect(passedOpts.lockfileDir).toBe(baseOpts.installDir)
      expect(passedOpts.dir).toBe(baseOpts.installDir)
    } finally {
      Object.defineProperty(process.stdin, 'isTTY', { value: wasTty, configurable: true })
    }
  })

  it('skips the prompt when there are no ignored builds', async () => {
    const { commands, calls } = makeCommands()
    await promptApproveGlobalBuilds({ ...baseOpts, ignoredBuilds: undefined }, commands)
    expect(calls).toHaveLength(0)
  })
})
