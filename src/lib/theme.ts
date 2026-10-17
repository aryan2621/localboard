import { useEffect } from 'react'
import { create } from 'zustand'

export type ThemeMode = 'light' | 'dark' | 'system'
export type FontSize = 'sm' | 'md' | 'lg'

const THEME_STORAGE_KEY = 'localboard-theme'
const FONT_SIZE_STORAGE_KEY = 'localboard-font-size'
const THEME_ORDER: ThemeMode[] = ['light', 'dark', 'system']
const FONT_SIZES: FontSize[] = ['sm', 'md', 'lg']

function readStored<T extends string>(key: string, allowed: readonly T[], fallback: T): T {
  try {
    const stored = localStorage.getItem(key)
    if (stored && (allowed as readonly string[]).includes(stored)) return stored as T
  } catch {
    // Storage can be unavailable; fall back for this session.
  }
  return fallback
}

function store(key: string, value: string) {
  try {
    localStorage.setItem(key, value)
  } catch {
    // The preference still applies for this session.
  }
}

function resolveTheme(mode: ThemeMode): 'light' | 'dark' {
  if (mode === 'system') return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
  return mode
}

function applyTheme(mode: ThemeMode) {
  const root = document.documentElement
  root.classList.remove('light', 'dark')
  root.classList.add(resolveTheme(mode))
}

type Appearance = {
  theme: ThemeMode
  fontSize: FontSize
  setTheme: (theme: ThemeMode) => void
  cycleTheme: () => void
  setFontSize: (size: FontSize) => void
}

export const useAppearance = create<Appearance>((set, get) => ({
  theme: readStored(THEME_STORAGE_KEY, THEME_ORDER, 'system'),
  fontSize: readStored(FONT_SIZE_STORAGE_KEY, FONT_SIZES, 'md'),
  setTheme: (theme) => {
    store(THEME_STORAGE_KEY, theme)
    applyTheme(theme)
    set({ theme })
  },
  cycleTheme: () => {
    const current = THEME_ORDER.indexOf(get().theme)
    get().setTheme(THEME_ORDER[(current + 1) % THEME_ORDER.length])
  },
  setFontSize: (fontSize) => {
    store(FONT_SIZE_STORAGE_KEY, fontSize)
    document.documentElement.dataset.fontSize = fontSize
    set({ fontSize })
  },
}))

/** Applies stored preferences and follows the OS theme while in system mode. Mount once. */
export function useAppearanceSync() {
  const theme = useAppearance((state) => state.theme)
  const fontSize = useAppearance((state) => state.fontSize)

  useEffect(() => {
    document.documentElement.dataset.fontSize = fontSize
  }, [fontSize])

  useEffect(() => {
    applyTheme(theme)
    if (theme !== 'system') return
    const media = window.matchMedia('(prefers-color-scheme: dark)')
    const onChange = () => applyTheme('system')
    media.addEventListener('change', onChange)
    return () => media.removeEventListener('change', onChange)
  }, [theme])
}
