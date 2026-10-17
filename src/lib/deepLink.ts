import { useNavigation } from '../stores/navigation'

/** `localboard://dashboard/<id>` and `localboard://chat/<id>` open that item; anything else is ignored. */
export function handleDeepLink(raw: string) {
  let url: URL
  try {
    url = new URL(raw)
  } catch {
    return
  }
  if (url.protocol !== 'localboard:') return
  const id = url.pathname.replace(/^\/+/, '').split('/')[0]
  const nav = useNavigation.getState()
  if (url.hostname === 'dashboard') nav.openDashboard(id || null)
  else if (url.hostname === 'chat' && id) nav.openConversation(id)
}

if ((window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__) {
  void import('@tauri-apps/api/event')
    .then(({ listen }) => listen<string>('deeplink://open', ({ payload }) => handleDeepLink(payload)))
    .catch((failure) => console.error('Deep links are unavailable:', failure))
}
