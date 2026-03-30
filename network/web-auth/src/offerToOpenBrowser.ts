export interface OfferToOpenBrowserReadlineInterface {
  once: (event: string, listener: () => void) => void
  close: () => void
}

export interface OfferToOpenBrowserExecFile {
  (file: string, args: readonly string[], callback: (error: Error | null) => void): unknown
}

export interface OfferToOpenBrowserContext {
  createReadlineInterface?: () => OfferToOpenBrowserReadlineInterface
  execFile?: OfferToOpenBrowserExecFile
  globalInfo: (message: string) => void
  globalWarn: (message: string) => void
  process: {
    platform?: string
    stdin: { isTTY?: boolean }
  }
}

export interface OfferToOpenBrowserParams {
  authUrl: string
  context: OfferToOpenBrowserContext
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
export async function offerToOpenBrowser ({
  authUrl,
  context,
  pollPromise,
}: OfferToOpenBrowserParams): Promise<string> {
  const { createReadlineInterface, execFile, globalInfo, globalWarn, process: proc } = context

  if (!createReadlineInterface || !execFile || !proc.stdin.isTTY) {
    return pollPromise
  }

  let cmd: string
  let args: string[]
  switch (proc.platform) {
    case 'darwin':
      cmd = 'open'
      args = [authUrl]
      break
    case 'win32':
      cmd = 'cmd'
      args = ['/c', 'start', '', authUrl]
      break
    case 'linux':
      cmd = 'xdg-open'
      args = [authUrl]
      break
    default:
      return pollPromise
  }

  let rl: OfferToOpenBrowserReadlineInterface
  try {
    rl = createReadlineInterface()
  } catch (err) {
    globalWarn(`Could not set up keyboard listener: ${String(err)}`)
    return pollPromise
  }

  globalInfo('Press ENTER to open the URL in your browser.')

  rl.once('line', () => {
    runExecFile(execFile, cmd, args).catch((err) => {
      globalWarn(`Could not open browser automatically: ${String(err)}`)
      globalInfo('Please open the URL shown above manually.')
    })
  })

  try {
    return await pollPromise
  } finally {
    rl.close()
  }
}

function runExecFile (
  execFile: OfferToOpenBrowserExecFile,
  cmd: string,
  args: string[]
): Promise<void> {
  return new Promise<void>((resolve, reject) => {
    execFile(cmd, args, (err) => {
      if (err) reject(err)
      else resolve()
    })
  })
}
