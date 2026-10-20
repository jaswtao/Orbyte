/** Author: jaswtao · Date: 2026-10-12 */
import { createApp } from 'vue'
import { createPinia } from 'pinia'
import App from './App.vue'

if (import.meta.env.PROD) {
  // 正式版阻止 WebView 的刷新和开发者工具快捷键，避免丢失未保存文档。
  // 不能阻止传播：终端仍需收到 Ctrl+R（反向历史搜索）、F5、F12 等按键。
  document.addEventListener('keydown', (event) => {
    if (event.key === 'F5' || event.key === 'F12' ||
      ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'r') ||
      (event.ctrlKey && event.shiftKey && event.key.toLowerCase() === 'i')) {
      event.preventDefault()
    }
  }, true)
  document.addEventListener('contextmenu', (event) => event.preventDefault())
}

if (import.meta.env.DEV && new URLSearchParams(location.search).has('probe')) {
  void import('./terminalProbe')
} else {
  // 样式就绪后挂载，避免首帧显示系统默认控件并触发颜色过渡。
  void import('./style.css').then(() => createApp(App).use(createPinia()).mount('#app'))
}
