export interface EnterKeyListener {
  enterPromise: Promise<void>
  cleanup: () => void
}

export interface PollWithBrowserOpenContext {
  listenForEnter?: () => EnterKeyListener
  openBrowser?: (url: string) => Promise<void>
  globalInfo: (message: string) => void
  globalWarn: (message: string) => void
}

export interface PollWithBrowserOpenParams {
  authUrl: string
  context: PollWithBrowserOpenContext
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
export async function pollWithBrowserOpen ({
  authUrl,
  context,
  pollPromise,
}: PollWithBrowserOpenParams): Promise<string> {
  const { listenForEnter, openBrowser, globalInfo, globalWarn } = context

  if (!listenForEnter || !openBrowser) {
    return pollPromise
  }

  let listener: EnterKeyListener
  try {
    listener = listenForEnter()
  } catch (err) {
    globalWarn(`Could not set up keyboard listener: ${String(err)}`)
    return pollPromise
  }

  globalInfo('Press ENTER to open in browser...')

  listener.enterPromise.then(() => {
    openBrowser(authUrl).catch((err) => {
      globalWarn(`Could not open browser automatically: ${String(err)}`)
      globalInfo('Please open the URL shown above manually.')
    })
  }, () => {})

  try {
    return await pollPromise
  } finally {
    listener.cleanup()
  }
}
