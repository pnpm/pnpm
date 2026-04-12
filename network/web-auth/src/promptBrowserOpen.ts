import open from 'open'

export interface PromptBrowserOpenReadlineInterface {
  once: (event: string, listener: () => void) => void
  close: () => void
}

export interface PromptBrowserOpenContext {
  createReadlineInterface?: () => PromptBrowserOpenReadlineInterface
  globalInfo: (message: string) => void
  globalWarn: (message: string) => void
  process: { stdin: { isTTY?: boolean } }
}

export interface PromptBrowserOpenParams {
  authUrl: string
  context: PromptBrowserOpenContext
  pollPromise: Promise<string>
}

/**
 * Wraps a token-polling promise with an optional "Press ENTER to open in
 * browser" prompt.
 *
 * While the poll runs in the background, listens for the user pressing Enter
 * to open the authentication URL in their browser.  When the poll completes
 * (regardless of whether the user pressed Enter), the keyboard listener is
 * cleaned up.
 *
 * Error-tolerant: failures in the keyboard listener or browser opening are
 * logged as warnings and do not interrupt the poll.
 */
export async function promptBrowserOpen ({
  authUrl,
  context,
  pollPromise,
}: PromptBrowserOpenParams): Promise<string> {
  const { createReadlineInterface, globalInfo, globalWarn, process } = context

  if (!createReadlineInterface || !process.stdin.isTTY) {
    return pollPromise
  }

  let rl: PromptBrowserOpenReadlineInterface
  try {
    rl = createReadlineInterface()
  } catch (err) {
    globalWarn(`Could not set up keyboard listener: ${String(err)}`)
    return pollPromise
  }

  globalInfo('Press ENTER to open the URL in your browser.')

  rl.once('line', () => {
    open(authUrl).catch((err: unknown) => {
      globalWarn(`Could not open browser automatically: ${String(err)}`)
      globalInfo('Please open the URL shown above manually.')
    })
  })

  // Only await pollPromise — do NOT await the Enter keypress.
  //
  // The Enter listener is a fire-and-forget side effect. Users may authenticate
  // on their phone (via QR code or pasted URL) without ever pressing Enter, so
  // the poll must be able to complete independently.
  //
  // npm uses Promise.all([opener, poll]) which blocks the entire flow until the
  // user presses Enter — even if authentication already succeeded on another
  // device: <https://github.com/npm/npm-profile/blob/d1a48be4/lib/index.js#L85-L98>
  try {
    return await pollPromise
  } finally {
    rl.close()
  }
}
