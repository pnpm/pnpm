import { expect, jest, test } from '@jest/globals'
import type { CommandHandlerMap } from '@pnpm/cli.command'
import type { DepPath } from '@pnpm/types'

import { promptApproveGlobalBuilds } from '../src/promptApproveGlobalBuilds.js'

test('promptApproveGlobalBuilds skips approve-builds in CI unless test auto-approve is set', async () => {
  const prevIsTty = process.stdin.isTTY
  const prevAutoApprove = process.env.PNPM_AUTO_APPROVE_BUILDS_FOR_TESTS
  Object.defineProperty(process.stdin, 'isTTY', { value: true, configurable: true })
  delete process.env.PNPM_AUTO_APPROVE_BUILDS_FOR_TESTS
  const approveBuilds = jest.fn<CommandHandlerMap['approve-builds']>().mockResolvedValue(undefined)
  const ignoredBuilds = new Set(['esbuild@1.0.0' as DepPath])

  try {
    await promptApproveGlobalBuilds({
      allowBuilds: {},
      globalPkgDir: '/global/v11',
      ignoredBuilds,
      inheritedOpts: { ci: true },
      installDir: '/global/v11/install',
    }, { 'approve-builds': approveBuilds })
    expect(approveBuilds).not.toHaveBeenCalled()

    process.env.PNPM_AUTO_APPROVE_BUILDS_FOR_TESTS = '1'
    await promptApproveGlobalBuilds({
      allowBuilds: {},
      globalPkgDir: '/global/v11',
      ignoredBuilds,
      inheritedOpts: { ci: true },
      installDir: '/global/v11/install',
    }, { 'approve-builds': approveBuilds })
    expect(approveBuilds).toHaveBeenCalledWith(expect.objectContaining({ all: true }), [], expect.any(Object))
  } finally {
    Object.defineProperty(process.stdin, 'isTTY', { value: prevIsTty, configurable: true })
    if (prevAutoApprove === undefined) {
      delete process.env.PNPM_AUTO_APPROVE_BUILDS_FOR_TESTS
    } else {
      process.env.PNPM_AUTO_APPROVE_BUILDS_FOR_TESTS = prevAutoApprove
    }
  }
})
