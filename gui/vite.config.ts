import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig(async () => ({
  plugins: [react()],
  base: '',
  clearScreen: false,
  server: { port: 5199, strictPort: true },
  envPrefix: ['VITE_', 'TAURI_'],
  build: { outDir: 'dist', emptyOutDir: true },
}))
