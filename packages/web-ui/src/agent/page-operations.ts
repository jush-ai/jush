/**
 * What the views may do on a program's page of one conversation, which the
 * program names (`crates/core-api/src/page.rs`): a control of an operation
 * shows only while the views may do it, and the rest of the view keeps its
 * place. In the web app every operation is allowed.
 */
import { shallowRef } from 'vue'
import { pageOperationSchema, type PageOperation } from '@jush-agent/protocol'

export type { PageOperation }

/** The operations allowed, or null for every one. */
const allowed = shallowRef<ReadonlySet<PageOperation> | null>(null)

/** Whether the views may do `operation`. */
export function may(operation: PageOperation): boolean {
  return allowed.value?.has(operation) ?? true
}

/**
 * Whether the views open the window `id`: one an operation names, as Files,
 * Changes, Browser and Simulators are, only with it.
 */
export function mayOpen(id: string): boolean {
  const named = pageOperationSchema.safeParse(id)
  return !named.success || may(named.data)
}

/** Lets the views do what `operations` names alone, or everything with null. */
export function allowOperations(operations: Iterable<PageOperation> | null): void {
  allowed.value = operations === null ? null : new Set(operations)
}
