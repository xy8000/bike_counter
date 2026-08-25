import { useEffect, useState } from 'react'
import { TopBar } from './TopBar'
import { SearchDialog } from '../search/SearchDialog'
import type { StationSummary } from '../stations/types'

/// The persistent top header plus the search dialog, shared by every route so
/// the header (brand, global summary, search trigger) stays active on the map
/// and on the station detail page. Each page supplies its own callbacks for
/// selecting / finding / opening a station result; any selection closes the
/// dialog.
export function SearchableHeader({
  onSelect,
  onFind,
  onDetail,
}: {
  onSelect: (station: StationSummary) => void
  onFind: (station: StationSummary) => void
  onDetail: (station: StationSummary) => void
}) {
  const [searchOpen, setSearchOpen] = useState(false)
  const close = () => setSearchOpen(false)

  // Esc closes the search dialog.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && searchOpen) {
        close()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [searchOpen])

  return (
    <>
      <TopBar onOpenSearch={() => setSearchOpen(true)} />
      {searchOpen && (
        <SearchDialog
          onClose={close}
          onSelect={(station) => {
            onSelect(station)
            close()
          }}
          onFind={(station) => {
            onFind(station)
            close()
          }}
          onDetail={(station) => {
            onDetail(station)
            close()
          }}
        />
      )}
    </>
  )
}
