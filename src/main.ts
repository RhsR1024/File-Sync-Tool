import { createApp } from 'vue'
import './style.css'
import App from './App.vue'
import router from './router'
import { i18n } from './i18n'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { markFrontendReady } from './lib/tauri'

// 创建Vue应用实例
const app = createApp(App)

// 使用路由
app.use(router)

// 使用i18n
app.use(i18n)

// 挂载应用
app.mount('#app')

// 前端存活标记：主窗口页面真正跑起来后写入 app.log。开机后“只有托盘图标、
// 窗口唤不出”的现场，若 app.log 缺这一行即可断定 webview 未加载成功。
if (getCurrentWindow().label === 'main') {
  markFrontendReady('main').catch(() => {
    // 纯诊断日志，失败（如浏览器直开的 pnpm dev）不影响应用
  })
}
