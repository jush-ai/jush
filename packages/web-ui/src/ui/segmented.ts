/**
 * The option a click on a segmented control leaves chosen. `clicked` is the
 * segment's value, or undefined for a click on the frame around and between the
 * segments.
 *
 * - Two options are one toggle: any click switches to the other, the chosen segment
 *   and the frame included, unless `exact`.
 * - With more, or `exact`, a click on a segment chooses it, and a click on the
 *   frame changes nothing.
 */
export function clickedChoice<T>(values: readonly T[], chosen: T, clicked: T | undefined, exact = false): T {
  if (values.length === 2 && !exact) {
    return values[0] === chosen ? values[1]! : values[0]!
  }
  return clicked ?? chosen
}
