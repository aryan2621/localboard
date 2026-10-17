import { Toaster as Sonner } from 'sonner'
import { useAppearance } from '../../lib/theme'

export function Toaster() {
  const theme = useAppearance((state) => state.theme)
  return (
    <Sonner
      theme={theme}
      position="bottom-right"
      className="toaster group"
      toastOptions={{
        classNames: {
          toast:
            'group toast group-[.toaster]:bg-popover group-[.toaster]:text-foreground group-[.toaster]:border-border group-[.toaster]:rounded-xl group-[.toaster]:shadow-[0_12px_32px_-8px_hsl(var(--foreground)/0.18)] group-[.toaster]:font-sans',
          description: 'group-[.toast]:text-muted-foreground',
          actionButton: 'group-[.toast]:bg-foreground group-[.toast]:text-background',
          cancelButton: 'group-[.toast]:bg-muted group-[.toast]:text-foreground',
        },
      }}
    />
  )
}
