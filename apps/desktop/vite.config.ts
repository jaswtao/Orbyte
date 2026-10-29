import { fileURLToPath, URL } from 'node:url'
import { readFileSync } from 'node:fs'
import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

const host = process.env.TAURI_DEV_HOST
const { version } = JSON.parse(readFileSync(new URL('./src-tauri/tauri.conf.json', import.meta.url), 'utf8'))
const releaseSummary = JSON.parse(readFileSync(new URL(`../../doc/releases/v${version}.json`, import.meta.url), 'utf8'))
if (releaseSummary.version !== version) throw new Error('Release summary must match the application version.')

export default defineConfig({
  define: { __ORBYTE_RELEASE__: JSON.stringify(releaseSummary) },
  plugins: [vue()],
  clearScreen: false,
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: 'ws', host, port: 1421 } : undefined,
    watch: {
      ignored: ['**/src-tauri/**'],
    },
  },
  envPrefix: ['VITE_', 'TAURI_ENV_*'],
  build: {
    target: 'es2022',
    rolldownOptions: {
      output: {
        codeSplitting: {
          // 将依赖一起分包，避免共享依赖留在应用入口，导致组件库反向引用入口。
          includeDependenciesRecursively: true,
          groups: [
            { name: 'vendor-vue', test: /node_modules[\\/](@vue|vue|pinia)[\\/]/ },
            { name: 'vendor-naive', test: /node_modules[\\/](naive-ui|vueuc|vooks|css-render|@css-render|seemly|treemate|evtd|vdirs|date-fns)[\\/]/ },
            { name: 'vendor-xterm', test: /node_modules[\\/]@xterm[\\/]/ },
            { name: 'vendor-codemirror', test: /node_modules[\\/](@codemirror[\\/](state|view|commands|language|search|autocomplete|lint)|@lezer[\\/](common|highlight|lr)|codemirror)[\\/]/ },
          ],
        },
      },
    },
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },
})
