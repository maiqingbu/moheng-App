import { defineConfig } from 'vitest/config'

export default defineConfig({
  test: {
    // Pinia store tests run under vitest (pure TS); the rest of the suite keeps
    // using `node --test` via `npm test`. Vitest is isolated to `tests/store`.
    include: ['tests/store/**/*.test.ts'],
    environment: 'node',
  },
})
