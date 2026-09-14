import { defineConfig } from 'vitest/config'
import vue from '@vitejs/plugin-vue'

export default defineConfig({
  plugins: [vue()],
  test: {
    // Frontend and Pinia tests run under vitest; the rest of the suite keeps
    // using `node --test` via `npm test`. Vitest remains isolated to `tests/store`.
    include: ['tests/store/**/*.test.ts'],
    environment: 'node',
  },
})
