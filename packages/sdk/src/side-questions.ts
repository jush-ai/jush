/**
 * A conversation's side questions: a question about a passage of an answer,
 * which its own agent answers beside the conversation.
 */
import { randomUUID } from 'node:crypto'
import { sideQuestionAnswerSchema, type AskRequest, type SideQuestionAnswer } from '@jush-agent/protocol'
import { content, type Input, type MessageOptions } from './input'
import { segment, type CoreRoutes } from './routes'

/**
 * A side question to ask: its passage and its question, and who asked it as
 * `senderId`, as a message names its sender; the SDK makes its id unless
 * named.
 */
export type SideQuestionAsk = Omit<AskRequest, 'id' | 'question'> & { id?: string; question: Input }

export interface SideQuestions {
  ask(question: SideQuestionAsk): Promise<SideQuestionAnswer>
  /** Asks the side question `side` more; `options.senderId` names who asked. */
  followUp(side: string, input: Input, options?: MessageOptions): Promise<void>
  retry(side: string): Promise<void>
  stop(side: string): Promise<void>
  /** Folds the side question `side`, or unfolds it. */
  fold(side: string, folded: boolean): Promise<SideQuestionAnswer>
  remove(side: string): Promise<void>
}

export function sideQuestions(routes: CoreRoutes, id: string): SideQuestions {
  const base = `/conversations/${segment(id)}/side-questions`
  const one = (side: string) => `${base}/${segment(side)}`
  return {
    ask: (question) => routes.json('POST', base, sideQuestionAnswerSchema, {
      ...question,
      id: question.id ?? randomUUID(),
      question: content(question.question),
    }),
    followUp: (side, input, options = {}) =>
      routes.send('POST', `${one(side)}/messages`, { content: content(input), senderId: options.senderId }),
    retry: (side) => routes.send('POST', `${one(side)}/retry`),
    stop: (side) => routes.send('POST', `${one(side)}/stop`),
    fold: (side, folded) => routes.json('PATCH', one(side), sideQuestionAnswerSchema, { folded }),
    remove: (side) => routes.send('DELETE', one(side)),
  }
}
