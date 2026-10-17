import { create } from 'zustand'
import type { AnalystEvent, Step } from '../lib/api'

/** Progress of an answer still being written; dropped once the stored message is refetched. */
export type LiveAnswer = { steps: Step[]; text: string }

type AnalystState = {
  live: Record<string, LiveAnswer>
  apply: (messageId: string, event: AnalystEvent) => void
  clear: (messageId: string) => void
}

const EMPTY: LiveAnswer = { steps: [], text: '' }

export const useAnalyst = create<AnalystState>((set) => ({
  live: {},
  apply: (messageId, event) =>
    set((state) => {
      const current = state.live[messageId] ?? EMPTY
      if (event.type === 'step') {
        const index = current.steps.findIndex((step) => step.id === event.step.id)
        const steps = index === -1 ? [...current.steps, event.step] : current.steps.map((step, i) => (i === index ? event.step : step))
        return { live: { ...state.live, [messageId]: { ...current, steps } } }
      }
      if (event.type === 'token') {
        return { live: { ...state.live, [messageId]: { ...current, text: current.text + event.text } } }
      }
      return state
    }),
  clear: (messageId) =>
    set((state) => {
      const live = { ...state.live }
      delete live[messageId]
      return { live }
    }),
}))
