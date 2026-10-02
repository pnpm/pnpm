import { createResources } from './resources.mjs'
import { openTerminal, writeTerminal } from './terminal-session.mjs'

export async function password (options, context = {}) {
  if (typeof options.message !== 'string') throw new TypeError('Password prompt message must be a string')
  context.signal?.throwIfAborted()
  const resources = createResources()
  try {
    const { handle } = await openTerminal(resources, { ...context, output: context.output ?? process.stderr, maxQueuedKeys: 65536 })
    const session = resources.get(handle, 'terminal')
    await writeTerminal(session, `${options.message}: `)
    return await readPassword(session, options.allowEmpty, context.signal)
  } finally {
    await resources.closeAll()
  }
}

async function readPassword (session, allowEmpty, signal) {
  const characters = []
  while (true) {
    // eslint-disable-next-line no-await-in-loop -- input must be consumed in key order
    const key = await session.read(signal)
    if (['ctrl-c', 'escape', 'eof'].includes(key.name)) throw Object.assign(new Error('Password input cancelled'), { code: 'EINTR' })
    if (key.name === 'enter' && (allowEmpty || characters.length)) {
      await writeTerminal(session, '\n') // eslint-disable-line no-await-in-loop -- newline is emitted only after accepting the complete password
      return characters.join('')
    }
    if (key.name === 'backspace' || key.name === 'delete') characters.pop()
    else if (key.character && Array.from(key.character).every(character => character >= ' ' && character !== '\x7f')) characters.push(key.character)
    if (characters.length > 65536) throw new Error('Password input exceeds the terminal limit')
  }
}
