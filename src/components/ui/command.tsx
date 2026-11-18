import * as React from 'react'
import { Command as CommandPrimitive } from 'cmdk'
import { Search } from 'lucide-react'
import { cn } from '../../lib/utils'

const Command = React.forwardRef<React.ElementRef<typeof CommandPrimitive>, React.ComponentPropsWithoutRef<typeof CommandPrimitive>>(({ className, ...props }, ref) => (
  <CommandPrimitive ref={ref} className={cn('flex w-full flex-col overflow-hidden rounded-lg bg-popover text-popover-foreground', className)} {...props} />
))
Command.displayName = 'Command'

const CommandInput = React.forwardRef<React.ElementRef<typeof CommandPrimitive.Input>, React.ComponentPropsWithoutRef<typeof CommandPrimitive.Input>>(({ className, ...props }, ref) => (
  <div className="flex items-center gap-2 border-b border-border px-3">
    <Search size={14} className="shrink-0 text-muted-foreground" aria-hidden />
    <CommandPrimitive.Input ref={ref} className={cn('h-10 w-full bg-transparent text-app-label outline-none placeholder:text-muted-foreground', className)} {...props} />
  </div>
))
CommandInput.displayName = 'CommandInput'

const CommandList = React.forwardRef<React.ElementRef<typeof CommandPrimitive.List>, React.ComponentPropsWithoutRef<typeof CommandPrimitive.List>>(({ className, ...props }, ref) => (
  <CommandPrimitive.List ref={ref} className={cn('max-h-80 overflow-y-auto p-1', className)} {...props} />
))
CommandList.displayName = 'CommandList'

const CommandEmpty = React.forwardRef<React.ElementRef<typeof CommandPrimitive.Empty>, React.ComponentPropsWithoutRef<typeof CommandPrimitive.Empty>>((props, ref) => (
  <CommandPrimitive.Empty ref={ref} className="px-3 py-6 text-app-caption text-muted-foreground" {...props} />
))
CommandEmpty.displayName = 'CommandEmpty'

const CommandGroup = React.forwardRef<React.ElementRef<typeof CommandPrimitive.Group>, React.ComponentPropsWithoutRef<typeof CommandPrimitive.Group>>(({ className, ...props }, ref) => (
  <CommandPrimitive.Group
    ref={ref}
    className={cn('overflow-hidden text-foreground [&_[cmdk-group-heading]]:px-2 [&_[cmdk-group-heading]]:py-1.5 [&_[cmdk-group-heading]]:text-app-caption [&_[cmdk-group-heading]]:text-muted-foreground', className)}
    {...props}
  />
))
CommandGroup.displayName = 'CommandGroup'

const CommandItem = React.forwardRef<React.ElementRef<typeof CommandPrimitive.Item>, React.ComponentPropsWithoutRef<typeof CommandPrimitive.Item>>(({ className, ...props }, ref) => (
  <CommandPrimitive.Item
    ref={ref}
    className={cn('flex cursor-default select-none flex-col rounded-md px-2 py-1.5 text-app-label outline-none data-[selected=true]:bg-accent data-[selected=true]:text-accent-foreground', className)}
    {...props}
  />
))
CommandItem.displayName = 'CommandItem'

export { Command, CommandInput, CommandList, CommandEmpty, CommandGroup, CommandItem }
