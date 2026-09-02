import { ChevronLeft, ChevronRight } from 'lucide-react'
import { cn } from '@/lib/utils'

/// The mid-height pull/push handle shared by the sidebar and the station
/// overview. It is an extension attached to a panel's right side that protrudes
/// outward like a tab to grab (rounded only on the right, so its left edge reads
/// as seamless with the panel). `collapsed` controls the icon/accessible name:
/// collapsed shows a right chevron ("Show station list" / pull to extend),
/// expanded shows a left chevron ("Hide station list" / push to shrink).
export function SidebarHandle({
  collapsed,
  onToggle,
  className,
}: {
  collapsed: boolean
  onToggle: () => void
  className?: string
}) {
  return (
    <button
      type="button"
      onClick={onToggle}
      title={collapsed ? 'Show station list (H)' : 'Hide station list (H)'}
      aria-label={collapsed ? 'Show station list' : 'Hide station list'}
      aria-expanded={!collapsed}
      className={cn(
        'absolute top-1/2 right-0 hidden h-20 w-7 -translate-y-1/2 translate-x-full cursor-pointer items-center justify-center rounded-r-lg border border-l-0 bg-background text-muted-foreground shadow-md outline-none transition-colors hover:bg-accent hover:text-accent-foreground focus-visible:border-ring focus-visible:ring-ring/50 focus-visible:ring-[3px] sm:flex',
        className,
      )}
    >
      {collapsed ? <ChevronRight className="h-4 w-4" /> : <ChevronLeft className="h-4 w-4" />}
    </button>
  )
}
