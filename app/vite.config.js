import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  build: {
    // lightningcss (дефолтный CSS-минификатор) стабильно вырезает непрефиксный
    // backdrop-filter, оставляя только -webkit-backdrop-filter — из-за этого
    // эффект блюра пропадал в боевой сборке на всех платформах. Пакета esbuild
    // как отдельной зависимости здесь нет (Vite/rolldown его не подтягивает
    // автоматически), поэтому просто отключаем минификацию CSS целиком —
    // для такого небольшого приложения разница в размере не критична, а
    // гарантия, что ничего не будет вырезано — полная.
    cssMinify: false,
  },
})
