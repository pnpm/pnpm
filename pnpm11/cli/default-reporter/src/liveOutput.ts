import type * as logs from '@pnpm/core-loggers'
import type { StreamParser } from '@pnpm/logger'
import createDiffer from 'ansi-diff'
import type * as Rx from 'rxjs'
import stringLength from 'string-length'

import { EOL } from './constants.js'

// ANSI "erase from cursor to end of display". Appended after each
// differential update so that anything an external process (e.g. an SSH
// passphrase prompt) wrote below the rendered frame is cleared.
const ERASE_TO_END_OF_DISPLAY = '\x1b[0J'

type Differ = ReturnType<typeof createDiffer>

interface LiveFrame {
  stream: NodeJS.WriteStream
  outputMaxWidth: number
  differ: Differ
  // The width the live differ wraps its frame at, so a resize can be noticed.
  differWidth: number
  // How many leading lines of the view have scrolled out of the differ's frame
  // and been committed to the scrollback, how many rows the frame the differ is
  // holding takes up, and whether it already outgrew the terminal it was drawn
  // on. See `commitOverflow`.
  committedLines: number
  renderedFrameRows: number
  renderedFrameOutgrewTerminal: boolean
  // Hold redraws while an interactive prompt owns the terminal (see PromptMessage).
  promptActive: boolean
}

/**
 * Renders `output$` to `stream`, redrawing only what changed between views.
 * Returns a function that stops rendering.
 */
export function renderLiveOutput (
  output$: Rx.Observable<string>,
  opts: {
    stream: NodeJS.WriteStream
    outputMaxWidth: number
    streamParser: StreamParser<logs.Log>
  }
): () => void {
  const { stream } = opts
  const write = stream.write.bind(stream)
  const frame = createLiveFrame(stream, opts.outputMaxWidth)
  const onLog = (log: logs.Log): void => {
    trackPrompt(frame, log)
  }
  opts.streamParser.on('data', onLog)
  const logUpdate = (view: string): void => {
    if (frame.promptActive) return
    write(renderView(frame, view))
  }
  const subscription = output$
    .subscribe({
      complete () {},
      error: (err) => {
        logUpdate(err.message)
      },
      next: logUpdate,
    })
  return () => {
    subscription.unsubscribe()
    opts.streamParser.removeListener('data', onLog)
  }
}

function createLiveFrame (stream: NodeJS.WriteStream, outputMaxWidth: number): LiveFrame {
  const differWidth = Math.max(1, stream.columns ?? outputMaxWidth)
  return {
    stream,
    outputMaxWidth,
    differ: createDiffer({ height: stream.rows, width: differWidth }),
    differWidth,
    committedLines: 0,
    renderedFrameRows: 0,
    renderedFrameOutgrewTerminal: false,
    promptActive: false,
  }
}

function resetDiffer (frame: LiveFrame): void {
  frame.differWidth = Math.max(1, frame.stream.columns ?? frame.outputMaxWidth)
  frame.differ = createDiffer({ height: frame.stream.rows, width: frame.differWidth })
}

function trackPrompt (frame: LiveFrame, log: logs.Log): void {
  if (log.name !== 'pnpm:prompt') return
  if ((log as logs.PromptLog).action === 'start') {
    frame.promptActive = true
  } else {
    frame.promptActive = false
    // Drop the differ's now-stale frame: the terminal below it changed while paused.
    resetDiffer(frame)
  }
}

/**
 * The text that updates the terminal from the previous view to `view`.
 */
function renderView (frame: LiveFrame, view: string): string {
  // A new line should always be appended in case a prompt needs to appear.
  // Without a new line the prompt will be joined with the previous output.
  // An example of such prompt may be seen by running: pnpm update --interactive
  if (!view.endsWith(EOL)) view += EOL
  const lines = view.slice(0, -EOL.length).split(EOL)
  const committed = commitOverflow(frame, lines)
  // The lines from `committedLines` on are already laid out contiguously in
  // the view, so the visible frame is a slice of it rather than a second copy.
  const visible = view.slice(viewOffsetOfLine(view, lines, frame.committedLines))
  // `\r` resets the column to 0 in case an external process (e.g. an SSH
  // passphrase prompt) left the cursor mid-line. `ansi-diff` then writes
  // only the differential — the characters that actually changed between
  // the previous frame and this one — so sticky blocks like the lockfile
  // verdict and deprecation warnings are not re-written on every progress
  // tick. `\x1b[K` erases trailing characters on the current line.
  return `\r${committed}${frame.differ.update(visible)}\x1b[K${ERASE_TO_END_OF_DISPLAY}`
}

/**
 * Hands the lines that no longer fit on screen over to the scrollback and
 * restarts the differ below them, returning the differential that performs
 * the handover.
 *
 * `ansi-diff` redraws by moving the cursor up from the end of its frame, so
 * it can only reach lines that are still on screen. A frame taller than the
 * terminal has scrolled its top away, and every later redraw then lands that
 * many rows too low — overwriting output above the frame instead of updating
 * it (pnpm/pnpm#14270). Committing the overflow keeps the frame within the
 * terminal, at the cost of no longer being able to revise what was committed.
 */
function commitOverflow (frame: LiveFrame, lines: string[]): string {
  if (Math.max(1, frame.stream.columns ?? frame.outputMaxWidth) !== frame.differWidth) {
    // The terminal was resized. The frame on screen has reflowed at the new
    // width, so every position the differ tracked against the old one is
    // wrong: start over below what is already there.
    resetDiffer(frame)
  }
  if (lines.length <= frame.committedLines) {
    // The view no longer reaches past what was committed — an error frame
    // replaces it rather than extending it. Render it whole, below.
    frame.committedLines = 0
    resetDiffer(frame)
    return ''
  }
  const rows = frame.stream.rows
  if (!rows) return ''
  // One row is left over for the cursor line that the trailing EOL puts
  // below the frame.
  const maxRows = Math.max(rows - 1, 1)
  const layout = layOutFrame(lines, { committedLines: frame.committedLines, width: frame.differWidth, maxRows })
  // A frame taller than the terminal has scrolled its own top away — whether
  // because a line outgrew the screen or because the window shrank under it —
  // so no cursor move reaches back into it, and growing the window again does
  // not bring it back. Start afresh below instead, reprinting rather than
  // revising, and leave the commit for the next frame, whose layout is one
  // this differ laid out itself.
  const cannotRevise = frame.renderedFrameOutgrewTerminal || frame.renderedFrameRows > maxRows
  if (cannotRevise || layout.firstVisible === frame.committedLines) {
    frame.renderedFrameRows = layout.uncommittedRows
    frame.renderedFrameOutgrewTerminal = layout.uncommittedRows > maxRows
    if (cannotRevise || frame.renderedFrameOutgrewTerminal) resetDiffer(frame)
    return ''
  }
  frame.renderedFrameRows = layout.frameRows
  frame.renderedFrameOutgrewTerminal = false
  // Shrinking the frame to just the overflow leaves those lines untouched
  // where they already are, erases the rest of the frame below them, and
  // parks the cursor on the next line — where the fresh differ starts.
  const handover = frame.differ.update(`${lines.slice(frame.committedLines, layout.firstVisible).join(EOL)}${EOL}`).toString()
  resetDiffer(frame)
  frame.committedLines = layout.firstVisible
  return handover
}

interface FrameLayout {
  // The first line that still fits on screen together with every line after it.
  firstVisible: number
  // The rows the lines from `firstVisible` on take up.
  frameRows: number
  // The rows every line that is not committed yet takes up.
  uncommittedRows: number
}

function layOutFrame (
  lines: string[],
  opts: { committedLines: number, width: number, maxRows: number }
): FrameLayout {
  const { committedLines, width, maxRows } = opts
  let uncommittedRows = 0
  for (let lineIndex = committedLines; lineIndex < lines.length; lineIndex++) {
    uncommittedRows += renderedRows(lines[lineIndex], width)
  }
  // The last line always stays in the frame — there would be nothing left to
  // redraw otherwise — so the walk upwards starts one line above it.
  let firstVisible = lines.length - 1
  let frameRows = renderedRows(lines[firstVisible], width)
  for (let lineIndex = firstVisible - 1; lineIndex >= committedLines; lineIndex--) {
    const lineRows = renderedRows(lines[lineIndex], width)
    if (frameRows + lineRows > maxRows) break
    frameRows += lineRows
    firstVisible = lineIndex
  }
  return { firstVisible, frameRows, uncommittedRows }
}

/**
 * Where the `index`-th of `lines` starts in the `view` they were split from.
 * Measured from the end, so a long committed prefix costs nothing.
 */
function viewOffsetOfLine (view: string, lines: string[], index: number): number {
  let trailing = 0
  for (let lineIndex = lines.length - 1; lineIndex >= index; lineIndex--) {
    trailing += lines[lineIndex].length + EOL.length
  }
  return view.length - trailing
}

/**
 * How many terminal rows `line` occupies once wrapped at `width`, counting the
 * escape sequences in it as zero-width. Never zero: an empty line still takes a
 * row. `width` is the terminal's own column count, clamped to at least one.
 */
function renderedRows (line: string, width: number): number {
  return Math.max(1, Math.ceil(stringLength(line) / width))
}
