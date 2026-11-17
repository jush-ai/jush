/**
 * Where this page's debugging stands: the link being made, the link an agent
 * runs code in the page with until `expiresAt`, or why it could not be made
 * (`DebugPageDialog.vue`).
 */
export type DebugPhase =
  | { status: 'starting' }
  | { status: 'on'; url: string; expiresAt: Date }
  | { status: 'failed'; error: unknown }
