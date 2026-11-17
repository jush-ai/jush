import { defineConfig } from 'tsdown'

// The workspace's packages the SDK uses are bundled, with their types, since
// none is published. Their declarations are emitted together (`eager`):
// emitted one file at a time, the bundled ones lost their exports.
export default defineConfig({
  entry: ['src/index.ts'],
  format: ['esm'],
  dts: { eager: true },
  clean: true,
  platform: 'node',
  deps: { alwaysBundle: [/^@jush-agent\//] },
})
