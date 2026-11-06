/**
 * What a debug run returned or threw, written as the backend reads it
 * (`crates/web-api-protocol/src/debug.rs`): plain JSON, every string well
 * formed, no deeper than `DEBUG_VALUE_DEPTH`, and no larger than a page sends
 * in one message.
 *
 * - An element is a short selector, another node its name, a window
 *   `"[Window]"`, a function `"[Function name]"`, a cycle `"[Circular]"`, and
 *   `undefined` null. Elements, nodes and errors of another frame are told by
 *   what they are, not by their frame's classes.
 * - Anything iterable, a `Map`, a `Set`, a `NodeList`, a typed array, is an
 *   array of what it yields; an error is its name, message and stack.
 * - `toJSON` is called once, as `JSON.stringify` calls it.
 * - The writing stops once it is larger than a page sends, so a large object
 *   graph does not hold the tab, and answers `too_large`.
 */
import { DEBUG_VALUE_DEPTH, MAX_PAGE_MESSAGE_BYTES, type RunOutcome } from '@jush-ai/protocol'

/** What a value is written as. */
type Written = null | boolean | number | string | Written[] | { [key: string]: Written }

/** The most classes a written element names, so that its selector stays short. */
const SELECTOR_CLASSES = 3

/** A lone surrogate, which a cut through an emoji leaves, and which no JSON reader takes. */
const LONE_SURROGATE = /[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/g

/** `text` with each lone surrogate replaced by U+FFFD, as a well-formed string has it. */
export function wellFormed(text: string): string {
  return text.replace(LONE_SURROGATE, '�')
}

/** Ends a writing that grew larger than a page sends; `bytes` is how far it got. */
class TooLarge {
  constructor(readonly bytes: number) {}
}

function isError(value: object): value is Error {
  const tag = Object.prototype.toString.call(value)
  return tag === '[object Error]' || tag === '[object DOMException]'
}

/** Whether `value` is a DOM node, of this frame or another. */
function isNode(value: object): value is Node {
  return typeof (value as Node).nodeType === 'number' && typeof (value as Node).nodeName === 'string'
}

/** A short selector of `element`: its tag, its id and its first classes. */
function selector(element: Element): string {
  const id = element.id === '' ? '' : `#${element.id}`
  const classes = [...element.classList].slice(0, SELECTOR_CLASSES).map((name) => `.${name}`).join('')
  return wellFormed(`${element.localName}${id}${classes}`)
}

/** Writes values, counting at least the characters their JSON takes. */
class Writer {
  private size = 0

  private count(characters: number): void {
    this.size += characters
    if (this.size > MAX_PAGE_MESSAGE_BYTES)
      throw new TooLarge(this.size)
  }

  private text(text: string): string {
    this.count(text.length + 2)
    return wellFormed(text)
  }

  /** `value`, held by `ancestors`; `converted` once its `toJSON` was called. */
  write(value: unknown, ancestors: readonly object[] = [], converted = false): Written {
    switch (typeof value) {
      case 'undefined':
        this.count(4)
        return null
      case 'boolean':
        this.count(5)
        return value
      case 'number':
        this.count(String(value).length)
        // As JSON writes them.
        return Number.isFinite(value) ? value : null
      case 'string':
        return this.text(value)
      case 'bigint':
        return this.text(`${value}n`)
      case 'symbol':
        return this.text(value.toString())
      case 'function':
        return this.text(`[Function ${value.name === '' ? 'anonymous' : value.name}]`)
    }
    if (value === null) {
      this.count(4)
      return null
    }
    const object = value as object
    if (ancestors.includes(object))
      return this.text('[Circular]')
    if (ancestors.length >= DEBUG_VALUE_DEPTH)
      return this.text('[Deep]')
    if (isNode(object))
      return this.text(object.nodeType === Node.ELEMENT_NODE ? selector(object as Element) : object.nodeName)
    if ('window' in object && object.window === object)
      return this.text('[Window]')
    const inner = [...ancestors, object]
    if (isError(object)) {
      return {
        name: this.text(String(object.name)),
        message: this.text(String(object.message)),
        stack: typeof object.stack === 'string' ? this.text(object.stack) : null,
      }
    }
    if (!converted && 'toJSON' in object && typeof object.toJSON === 'function')
      return this.write(object.toJSON(), ancestors, true)
    if (Symbol.iterator in object) {
      const items: Written[] = []
      for (const item of object as Iterable<unknown>) {
        this.count(1)
        items.push(this.write(item, inner))
      }
      return items
    }
    const fields: Record<string, Written> = {}
    for (const [key, item] of Object.entries(object)) {
      fields[this.text(key)] = this.write(item, inner)
    }
    return fields
  }
}

/** What a thrown value comes back as: an error's own words, any other value written. */
function threw(thrown: unknown): RunOutcome {
  if (typeof thrown === 'object' && thrown !== null && isError(thrown)) {
    return {
      kind: 'threw',
      name: wellFormed(String(thrown.name)),
      message: wellFormed(String(thrown.message)),
      stack: typeof thrown.stack === 'string' ? wellFormed(thrown.stack) : null,
    }
  }
  return { kind: 'threw', name: 'Thrown value', message: JSON.stringify(new Writer().write(thrown)), stack: null }
}

/**
 * What came of a run: what it `returned`, or what it `thrown` when
 * `returned` is absent. It never throws: a value it cannot write comes back
 * as the reason.
 */
export function outcome(result: { returned: unknown } | { thrown: unknown }): RunOutcome {
  try {
    return 'returned' in result
      ? { kind: 'returned', value: new Writer().write(result.returned) }
      : threw(result.thrown)
  } catch (error) {
    if (error instanceof TooLarge)
      return { kind: 'too_large', bytes: error.bytes }
    // A getter or a `toJSON` that threw, or a value too deep for the stack.
    const reason = error instanceof Error ? error.message : 'unknown'
    return {
      kind: 'threw',
      name: 'Unwritable',
      message: wellFormed(`The page could not write the value as JSON: ${reason}`),
      stack: null,
    }
  }
}
