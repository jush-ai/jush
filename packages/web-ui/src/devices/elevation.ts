/**
 * Whether a device's Windows runner runs elevated, as the user's own account
 * with its administrator rights, as its runner last said
 * (`crates/runner/src/elevation.rs`):
 *
 * - `elevated`: it does.
 * - `elevatable`: it runs with the account's limited rights, and Request
 *   shows a User Account Control prompt on the device's screen.
 * - `unavailable`: the account is not an administrator.
 */
export type Elevation = 'elevated' | 'elevatable' | 'unavailable'
