import { useMutation, useQuery } from '@tanstack/react-query'
import { toast } from 'sonner'
import { api, errorMessage, type WorkspaceList } from './api'
import { invalidateWorkspaceData, queryClient, queryKeys } from './queryClient'
import { useNavigation } from '../stores/navigation'

export function useWorkspaces() {
  return useQuery({ queryKey: queryKeys.workspaces, queryFn: api.workspaces })
}

/** Runs a workspace command; if it changed the open workspace, resets workspace-scoped views. */
export function useWorkspaceAction() {
  const resetForWorkspace = useNavigation((state) => state.resetForWorkspace)
  return useMutation({
    mutationFn: (run: () => Promise<WorkspaceList>) => run(),
    onSuccess: (list) => {
      const previous = queryClient.getQueryData<WorkspaceList>(queryKeys.workspaces)
      queryClient.setQueryData(queryKeys.workspaces, list)
      if (previous?.activeId !== list.activeId) {
        resetForWorkspace()
        invalidateWorkspaceData()
      }
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
}
