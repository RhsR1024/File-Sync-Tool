import { createApp } from 'vue'
import './style.css'
import App from './App.vue'
import router from './router'
import { i18n } from './i18n'

// 创建Vue应用实例
const app = createApp(App)

// 使用路由
app.use(router)

// 使用i18n
app.use(i18n)

// 挂载应用
app.mount('#app')
