import { useEffect } from 'react'
import { toast } from 'sonner'
import { api, errorMessage } from './api'
import { queryClient, queryKeys } from './queryClient'

export async function lockNow() {
  try {
    const locked = await api.lockApp()
    if (!locked) {
      toast.info('Set an app lock passphrase in Settings first')
      return
    }
    void queryClient.invalidateQueries({ queryKey: queryKeys.status })
    void queryClient.invalidateQueries({ queryKey: queryKeys.governance })
  } catch (failure) {
    toast.error(errorMessage(failure))
  }
}

/** Cmd/Ctrl-L locks the app. */
export function useLockShortcut() {
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && !event.shiftKey && !event.altKey && event.key.toLowerCase() === 'l') {
        event.preventDefault()
        void lockNow()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])
}
