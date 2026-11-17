import { isRunning } from '../agent/conversation-status'
import { conversationDotTone, NEWS_TONES, type DotSource, type NewsTone } from './conversation-dot'

/**
 * The sidebar's unread entry: how many of the workspace's conversations wear
 * a dot that says something is new, or else how many run, and the one its
 * click shows next. It reads the rows' dots (`conversationDotTone`), the rule
 * the rows, the tiles and the tab read.
 */
export type UnreadEntry =
  | { kind: 'unread'; count: number; tone: NewsTone }
  | { kind: 'running'; count: number; working: boolean }
  | { kind: 'idle' }
  /** The list is not there yet, or failed: the entry says nothing it does not know. */
  | { kind: 'unknown' }

/** What the entry reads of a row. */
export type EntrySource = { id: string; updatedAt: string } & DotSource

/** Where a conversation stands in the entry's order: how urgent its news is, the most urgent 0, and when it was last active. */
export interface EntryPlace {
  id: string
  rank: number
  updatedAt: string
}

/** How urgent a row's news is, the most urgent 0; null for none. */
function newsRank(row: DotSource): number | null {
  const at = NEWS_TONES.indexOf(conversationDotTone(row) as NewsTone)
  return at >= 0 ? at : null
}

/** Whether `a` comes before `b`: the more urgent, then the later active. */
function compare(a: EntryPlace, b: EntryPlace): number {
  return a.rank - b.rank || b.updatedAt.localeCompare(a.updatedAt)
}

/**
 * The places the entry goes through, in its order: those with news, the most
 * urgent and then the latest first, else those that run, the latest first.
 */
function lineUp(rows: readonly EntrySource[]): { kind: 'unread' | 'running'; places: EntryPlace[] } {
  const place = (row: EntrySource, rank: number): EntryPlace => ({ id: row.id, rank, updatedAt: row.updatedAt })
  const news = rows.flatMap((row) => {
    const rank = newsRank(row)
    return rank === null ? [] : [place(row, rank)]
  })
  if (news.length > 0)
    return { kind: 'unread', places: news.toSorted(compare) }
  return { kind: 'running', places: rows.filter((row) => isRunning(row.status)).map((row) => place(row, 0)).toSorted(compare) }
}

/** What the entry says of `rows`. */
export function unreadEntry(rows: readonly EntrySource[]): UnreadEntry {
  const { kind, places } = lineUp(rows)
  if (places.length === 0)
    return { kind: 'idle' }
  return kind === 'unread'
    ? { kind, count: places.length, tone: NEWS_TONES[places[0].rank] }
    : { kind, count: places.length, working: rows.some((row) => row.status === 'active') }
}

/**
 * The place the entry's click goes to while `shownId` shows: the one after
 * the shown one in the entry's order, and after the last the first. A shown
 * conversation that left the order, as one read, is placed where `last`, the
 * place the entry went to before, says it stood, so the next one is the one
 * after it still. Null while there is none.
 */
export function nextOf(rows: readonly EntrySource[], shownId: string | null, last: EntryPlace | null): EntryPlace | null {
  const { places } = lineUp(rows)
  if (places.length === 0)
    return null
  const at = places.findIndex((place) => place.id === shownId)
  if (at >= 0)
    return places[(at + 1) % places.length]
  const stood = last !== null && last.id === shownId ? last : null
  return (stood && places.find((place) => compare(place, stood) > 0)) ?? places[0]
}
