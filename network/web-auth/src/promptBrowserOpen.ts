export interface PromptBrowserOpenReadlineInterface {
  once: (event: string, listener: () => void) => void
  close: () => void
}

export interface PromptBrowserOpenExecFile {
  (file: string, args: readonly string[], callback: (error: Error | null) => void): unknown
}

export interface PromptBrowserOpenProcess {
  platform?: NodeJS.Platform
  stdin: { isTTY?: boolean }
}

export interface PromptBrowserOpenContext {
  createReadlineInterface?: () => PromptBrowserOpenReadlineInterface
  execFile?: PromptBrowserOpenExecFile
  globalInfo: (message: string) => void
  globalWarn: (message: string) => void
  process: PromptBrowserOpenProcess
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
  const { createReadlineInterface, execFile, globalInfo, globalWarn, process } = context

  if (!createReadlineInterface || !execFile || !process.stdin.isTTY) {
    return pollPromise
  }

  // Validate the URL before passing it to a shell command. On Windows,
  // cmd.exe re-parses execFile arguments and would interpret shell
  // metacharacters (&, |, etc.) in the URL as operators.
  let parsedUrl: URL
  try {
    parsedUrl = new URL(authUrl)
  } catch {
    return pollPromise
  }
  if (parsedUrl.protocol !== 'https:' && parsedUrl.protocol !== 'http:') {
    return pollPromise
  }

  let cmd: string
  let args: string[]
  switch (process.platform) {
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

  let rl: PromptBrowserOpenReadlineInterface
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

function runExecFile (
  execFile: PromptBrowserOpenExecFile,
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
