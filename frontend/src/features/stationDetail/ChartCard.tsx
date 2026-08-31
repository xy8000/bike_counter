import type { ReactNode } from 'react'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'

/// A bordered card wrapping one chart plus its title, optional subtitle and an
/// optional info note (used e.g. to explain the 30-day window).
export function ChartCard({
  title,
  subtitle,
  note,
  children,
}: {
  title: string
  subtitle?: string
  note?: string
  children: ReactNode
}) {
  return (
    <Card className="gap-2">
      <CardHeader className="px-4 pb-1 pt-4">
        <CardTitle className="text-base">{title}</CardTitle>
        {subtitle && <CardDescription>{subtitle}</CardDescription>}
      </CardHeader>
      <CardContent className="px-4 pb-4">
        {children}
        {note && <p className="mt-3 text-xs text-muted-foreground">{note}</p>}
      </CardContent>
    </Card>
  )
}
