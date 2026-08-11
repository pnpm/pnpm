import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterEach, beforeEach, expect, it, jest } from '@jest/globals'

jest.unstable_mockModule('@inquirer/prompts', () => {
  class Separator {
    separator: string
    readonly type = 'separator' as const
    constructor (separator: string) {
      this.separator = separator
    }
  }
  return {
    Separator,
    checkbox: jest.fn(),
    confirm: jest.fn(),
    input: jest.fn(),
    password: jest.fn(),
    select: jest.fn(),
  }
})
const { checkbox } = await import('@inquirer/prompts')
const { change } = await import('../../src/index.js')

const mockCheckbox = jest.mocked(checkbox)

let tempDir: string

beforeEach(() => {
  tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-change-cancel-test-'))
  fs.writeFileSync(path.join(tempDir, 'pnpm-workspace.yaml'), 'packages:\n  - packages/*\n')
})

afterEach(() => {
  fs.rmSync(tempDir, { recursive: true, force: true })
  mockCheckbox.mockReset()
})

it('change leaves without an error when the prompt is canceled', async () => {
  const rootDir = path.join(tempDir, 'packages', 'lib')
  fs.mkdirSync(rootDir, { recursive: true })
  const manifest = { name: 'lib', version: '1.0.0' }
  fs.writeFileSync(path.join(rootDir, 'package.json'), JSON.stringify(manifest, null, 2))

  const canceled = new Error('User force closed the prompt')
  canceled.name = 'ExitPromptError'
  mockCheckbox.mockRejectedValue(canceled)
  // `process.exit()` never returns in production, so the spy throws to stop
  // the handler where the real call would.
  const exited = new Error('process.exit')
  const exitSpy = jest.spyOn(process, 'exit').mockImplementation(() => {
    throw exited
  })

  try {
    await expect(
      change.handler({ dir: tempDir, workspaceDir: tempDir, allProjects: [{ rootDir, manifest }] } as any, []) // eslint-disable-line @typescript-eslint/no-explicit-any
    ).rejects.toBe(exited)
    expect(exitSpy).toHaveBeenCalledWith(0)
  } finally {
    exitSpy.mockRestore()
  }
})
