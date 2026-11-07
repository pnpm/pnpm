import {
  type LogBase,
  logger,
} from '@pnpm/logger'

export const promptLogger = logger<PromptMessage>('prompt')

export interface PromptMessage {
  /**
   * Emitted around an interactive prompt so the default reporter can hold its
   * progress redraws while the prompt owns the terminal — otherwise the next
   * redraw erases the prompt rendered below the frame (pnpm/pnpm#13019).
   */
  action: 'start' | 'end'
}

export type PromptLog = { name: 'pnpm:prompt' } & LogBase & PromptMessage
