import React from 'react'
import { createRoot } from 'react-dom/client'
import { QueryClientProvider } from '@tanstack/react-query'
import App from './App'
import { queryClient } from './lib/queryClient'
import './lib/deepLink'
import './index.css'

if (/Mac/i.test(navigator.platform) || /Mac/i.test(navigator.userAgent)) {
  document.documentElement.classList.add('platform-macos')
}

createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <App />
    </QueryClientProvider>
  </React.StrictMode>,
)

void Promise.all([
  import('@fontsource-variable/inter'),
  import('@fontsource-variable/source-serif-4'),
  import('@fontsource-variable/jetbrains-mono'),
])
