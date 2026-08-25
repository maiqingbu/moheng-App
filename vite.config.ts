import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

export default defineConfig({
  plugins: [vue()],
  server: { port: 1430, strictPort: true },
  build: { target: 'es2022', sourcemap: true }
})
