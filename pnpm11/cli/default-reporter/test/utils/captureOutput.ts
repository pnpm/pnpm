import { stripVTControlCharacters } from 'node:util'

import { toOutput$ } from '@pnpm/cli.default-reporter'
import { createStreamParser } from '@pnpm/logger'
import normalizeNewline from 'normalize-newline'

export async function captureOutput (
  opts: Omit<Parameters<typeof toOutput$>[0], 'streamParser'>,
  emit: () => void
): Promise<string[]> {
  const output: string[] = []
  const subscription = toOutput$({ ...opts, streamParser: createStreamParser() })
    .subscribe((message) => output.push(normalizeNewline(stripVTControlCharacters(message))))
  try {
    // toOutput$ registers its parser listener in a zero-delay timer.
    await new Promise<void>((resolve) => setTimeout(resolve, 0))
    emit()
    return output
  } finally {
    subscription.unsubscribe()
  }
}
