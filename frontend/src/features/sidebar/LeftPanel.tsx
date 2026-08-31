import type { ReactNode } from 'react'
import { cn } from '@/lib/utils'
import { SidebarHandle } from './SidebarHandle'

/// The generic left overlay panel. It holds either the station list or the
/// station overview and slides in/out as a whole: the mid-height pull/push
/// handle collapses/expands WHATEVER content is currently shown, so the panel
/// re-opens with the same content it had when it was closed.
export function LeftPanel({
  collapsed,
  onToggle,
  children,
}: {
  collapsed: boolean
  onToggle: () => void
  children: ReactNode
}) {
  return (
    <aside
      className={cn(
        'absolute inset-y-0 left-0 z-[500] flex w-[360px] min-h-0 flex-col border-r bg-background shadow-lg',
        'transition-transform duration-300 ease-in-out',
        collapsed ? '-translate-x-full' : 'translate-x-0',
      )}
    >
      {children}
      <SidebarHandle collapsed={collapsed} onToggle={onToggle} />
    </aside>
  )
}
