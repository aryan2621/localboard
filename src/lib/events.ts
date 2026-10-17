import { useEffect } from 'react'
import { listen } from '@tauri-apps/api/event'
import { toast } from 'sonner'
import { useGeneration } from '../stores/generation'
import { handleAnalystEvent } from './analyst'
import type { AnalystEventPayload, DownloadProgress, ModelCatalog, Step } from './api'
import { invalidateConnectionData, invalidateWorkspaceData, queryClient, queryKeys } from './queryClient'

const ANALYST_EVENT = 'analyst://event'
const GENERATE_PROGRESS = 'generate://progress'
const CATALOG_CHANGED = 'catalog://changed'
const DOWNLOAD_PROGRESS = 'llm://download-progress'
const DASHBOARD_REFRESHED = 'dashboard://refreshed'
const AUTOMATION_RAN = 'automation://ran'
const APP_READY = 'app://ready'

/** Bridges backend events into the query cache. Mount once at the app root. */
export function useBackendEvents() {
  useEffect(() => {
    if (!(window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__) return
    const stops: Promise<() => void>[] = [
      listen<string>(CATALOG_CHANGED, () => invalidateConnectionData()),
      listen<AnalystEventPayload>(ANALYST_EVENT, ({ payload }) => handleAnalystEvent(payload)),
      listen<Step>(GENERATE_PROGRESS, ({ payload }) => useGeneration.getState().apply(payload)),
      listen<{ id: string }>(DASHBOARD_REFRESHED, ({ payload }) => {
        void queryClient.invalidateQueries({ queryKey: queryKeys.dashboard(payload.id) })
        void queryClient.invalidateQueries({ queryKey: ['audit'] })
      }),
      listen<{ id: string }>(AUTOMATION_RAN, ({ payload }) => {
        void queryClient.invalidateQueries({ queryKey: queryKeys.automations })
        void queryClient.invalidateQueries({ queryKey: queryKeys.automationRuns(payload.id) })
        void queryClient.invalidateQueries({ queryKey: ['audit'] })
      }),
      listen(APP_READY, () => {
        void queryClient.invalidateQueries({ queryKey: queryKeys.status })
      }),
      listen<DownloadProgress>(DOWNLOAD_PROGRESS, ({ payload }) => {
        queryClient.setQueryData<ModelCatalog>(queryKeys.models, (current) =>
          current ? { ...current, download: payload } : current,
        )
        if (payload.finished) {
          toast.success('Model downloaded and verified')
          void queryClient.invalidateQueries({ queryKey: queryKeys.models })
        } else if (payload.error) {
          toast.error(payload.error)
          void queryClient.invalidateQueries({ queryKey: queryKeys.models })
        }
      }),
    ]
    return () => {
      for (const stop of stops) void stop.then((unlisten) => unlisten())
    }
  }, [])
}
