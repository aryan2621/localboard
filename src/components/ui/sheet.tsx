import * as React from 'react'
import * as DialogPrimitive from '@radix-ui/react-dialog'
import { X } from 'lucide-react'
import { Button } from './button'
import { cn } from '../../lib/utils'

/** A panel that slides in from the right, for inspecting or editing one item without leaving the page. */
const Sheet = DialogPrimitive.Root
const SheetTrigger = DialogPrimitive.Trigger
const SheetClose = DialogPrimitive.Close

const SheetContent = React.forwardRef<
  React.ElementRef<typeof DialogPrimitive.Content>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Content> & { width?: 'md' | 'lg' | 'xl'; overlay?: boolean }
>(({ className, children, width = 'md', overlay = true, ...props }, ref) => (
  <DialogPrimitive.Portal>
    {overlay && <DialogPrimitive.Overlay className="fixed inset-0 z-50 bg-[hsl(var(--shadow-color)/0.25)] data-[state=open]:animate-overlay-in data-[state=closed]:animate-overlay-out" />}
    <DialogPrimitive.Content
      ref={ref}
      className={cn(
        'fixed inset-y-0 right-0 z-50 flex h-full w-full flex-col border-l border-border bg-popover shadow-lg outline-none data-[state=open]:animate-sheet-in data-[state=closed]:animate-sheet-out',
        width === 'xl' ? 'max-w-4xl' : width === 'lg' ? 'max-w-2xl' : 'max-w-md',
        className,
      )}
      {...props}
    >
      {children}
      <DialogPrimitive.Close className="absolute right-4 top-4 rounded-md p-1 text-muted-foreground transition-colors duration-fast hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50">
        <X className="h-4 w-4" />
        <span className="sr-only">Close</span>
      </DialogPrimitive.Close>
    </DialogPrimitive.Content>
  </DialogPrimitive.Portal>
))
SheetContent.displayName = 'SheetContent'

function SheetHeader({ title, description, children }: { title: React.ReactNode; description?: React.ReactNode; children?: React.ReactNode }) {
  return (
    <div className="space-y-1 border-b border-border px-6 pb-4 pr-12 pt-5">
      <DialogPrimitive.Title className="font-serif text-app-title font-medium text-foreground">{title}</DialogPrimitive.Title>
      {description ? (
        <DialogPrimitive.Description className="text-app-caption text-muted-foreground">{description}</DialogPrimitive.Description>
      ) : (
        <DialogPrimitive.Description className="sr-only">Details</DialogPrimitive.Description>
      )}
      {children}
    </div>
  )
}

function SheetBody({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) {
  return <div className={cn('min-h-0 flex-1 space-y-5 overflow-y-auto px-6 py-5', className)} {...props} />
}

function SheetFooter({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) {
  return <div className={cn('flex items-center justify-end gap-2 border-t border-border px-6 py-3', className)} {...props} />
}

/** A drawer holding one form: title, fields, and Cancel / submit at the bottom. Used for creating and editing. */
function FormSheet({
  open,
  onOpenChange,
  title,
  description,
  submitLabel,
  onSubmit,
  submitting,
  canSubmit = true,
  width = 'md',
  children,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  title: React.ReactNode
  description?: React.ReactNode
  submitLabel: string
  onSubmit: () => void
  submitting?: boolean
  canSubmit?: boolean
  width?: 'md' | 'lg' | 'xl'
  children: React.ReactNode
}) {
  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent width={width}>
        <SheetHeader title={title} description={description} />
        <form
          className="flex min-h-0 flex-1 flex-col"
          onSubmit={(event) => {
            event.preventDefault()
            if (canSubmit && !submitting) onSubmit()
          }}
        >
          <SheetBody>{children}</SheetBody>
          <SheetFooter>
            <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button type="submit" loading={submitting} disabled={!canSubmit}>
              {submitLabel}
            </Button>
          </SheetFooter>
        </form>
      </SheetContent>
    </Sheet>
  )
}

export { Sheet, SheetTrigger, SheetClose, SheetContent, SheetHeader, SheetBody, SheetFooter, FormSheet }
