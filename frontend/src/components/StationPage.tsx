import type { MouseEvent, ReactNode } from 'react'
import { Link } from 'react-router-dom'
import { ArrowLeft } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { SearchableHeader } from '../features/header/SearchableHeader'
import { useStationActions } from '../features/stations/useStationActions'

/// The shared chrome for the station-focused pages (station detail, station
/// summary and the data-sources pages): the persistent header + search and a
/// centred scrolling column with a back-link row and an optional error line.
export function StationPage({
  backTo,
  backLabel,
  errorMessage,
  backOnClick,
  children,
}: Readonly<{
  backTo: string
  backLabel: ReactNode
  errorMessage?: string
  backOnClick?: (event: MouseEvent<HTMLAnchorElement>) => void
  children: ReactNode
}>) {
  const { openDetail, findOnMap } = useStationActions()

  return (
    <div className="flex h-screen flex-col">
      <SearchableHeader onSelect={openDetail} onFind={findOnMap} onDetail={openDetail} />

      <main className="min-h-0 flex-1 overflow-y-auto">
        <div className="mx-auto max-w-6xl px-4 py-4 sm:py-6">
          <div className="mb-4 flex items-center justify-between gap-4">
            <Button asChild variant="outline" size="sm">
              <Link to={backTo} onClick={backOnClick}>
                <ArrowLeft /> {backLabel}
              </Link>
            </Button>
            {errorMessage && (
              <span className="text-sm font-semibold text-destructive">{errorMessage}</span>
            )}
          </div>
          {children}
        </div>
      </main>
    </div>
  )
}
