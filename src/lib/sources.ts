import { useMutation, useMutationState } from '@tanstack/react-query'
import { open } from '@tauri-apps/plugin-dialog'
import { toast } from 'sonner'
import { api, DATA_EXTENSIONS, errorMessage } from './api'
import { invalidateConnectionData, queryClient, queryKeys } from './queryClient'

type AddRequest = { paths: string[]; recursive: boolean }

const ADD_SOURCES_KEY = ['addSources'] as const
const PROGRESS_TOAST_ID = 'add-sources-progress'

function baseName(path: string) {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path
}

/**
 * Connects files or folders one by one so a bad path does not block the rest, then reports
 * one combined toast. Progress is shown in a single loading toast that updates per source.
 */
export function useAddSources() {
  return useMutation({
    mutationKey: ADD_SOURCES_KEY,
    mutationFn: async ({ paths, recursive }: AddRequest) => {
      let tables = 0
      const failures: string[] = []
      const warnings: string[] = []
      for (const [index, path] of paths.entries()) {
        const position = paths.length > 1 ? ` (${index + 1} of ${paths.length})` : ''
        toast.loading(`Reading ${baseName(path)}${position}`, {
          id: PROGRESS_TOAST_ID,
          description: 'Registering the table and profiling columns.',
        })
        try {
          const { report } = await api.addFileSource(path, recursive)
          tables += report.tables
          warnings.push(...report.errors)
        } catch (failure) {
          failures.push(errorMessage(failure))
        }
      }
      return { tables, failures, warnings }
    },
    onSuccess: ({ tables, failures, warnings }) => {
      invalidateConnectionData()
      if (tables > 0) {
        toast.success(`${tables} ${tables === 1 ? 'table' : 'tables'} added`, { id: PROGRESS_TOAST_ID, description: null })
      } else {
        toast.dismiss(PROGRESS_TOAST_ID)
      }
      for (const message of [...failures, ...warnings]) toast.error(message)
    },
    onError: (failure) => {
      toast.dismiss(PROGRESS_TOAST_ID)
      toast.error(errorMessage(failure))
    },
  })
}

/** Paths currently being added from anywhere in the app (buttons, onboarding or window drops). */
export function usePendingSources(): string[] {
  return useMutationState({
    filters: { mutationKey: ADD_SOURCES_KEY, status: 'pending' },
    select: (mutation) => (mutation.state.variables as AddRequest | undefined)?.paths ?? [],
  }).flat()
}

export async function pickSources(directory: boolean): Promise<string[]> {
  const picked = await open({
    multiple: !directory,
    directory,
    filters: directory ? undefined : [{ name: 'Data files', extensions: [...DATA_EXTENSIONS] }],
  })
  if (picked === null) return []
  return Array.isArray(picked) ? picked : [picked]
}
