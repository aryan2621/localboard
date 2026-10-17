import { create } from 'zustand'
import type { Step } from '../lib/api'

/** Progress of the dashboard being built right now; emptied when a new run starts. */
type GenerationState = {
  steps: Step[]
  apply: (step: Step) => void
  reset: () => void
}

export const useGeneration = create<GenerationState>((set) => ({
  steps: [],
  apply: (step) =>
    set((state) => {
      const index = state.steps.findIndex((existing) => existing.id === step.id)
      return { steps: index === -1 ? [...state.steps, step] : state.steps.map((existing, i) => (i === index ? step : existing)) }
    }),
  reset: () => set({ steps: [] }),
}))
