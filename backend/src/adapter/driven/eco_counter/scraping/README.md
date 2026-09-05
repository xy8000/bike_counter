# Eco-Counter ScreenScraping mode (`eco_counter` → `scraping`)

**Scaffold.** One of the three switchable modes of the `eco_counter` adapter
(see the parent [`README.md`](../README.md)), enabled by listing
`screen_scraping` in the data source's `modes` var.

Eco-Counter exposes some counter data only through **public web views** that
are accessible in a browser but have no usable API (the migrated `*.eco-counter.com`
dashboards / the classic `data.eco-counter.com` pages). This mode is the place
for a scraper of such a view. The structure is in place (config, a page-fetching
[`client.rs`](client.rs), and a [`DataProvider`](../../../../../src/core/domain/data_source/provider_port.rs:151)
shell) but the **page parser is not implemented yet**, so the provider currently
serves **no stations** and emits a one-time `WARNING` when it runs.

Configuration (vars read with the `web_` mode prefix):

| Var | Required | Default | Meaning |
|---|---|---|---|
| `web_scrape_url` | no | `https://data.eco-counter.com/ParcPublic/?id=1` | the public page to scrape |

To implement it: point `web_scrape_url` at the concrete accessible view, fetch it
with [`PageClient::fetch_page`](client.rs:13), parse the stations/channels and
serve measurements here, mirroring the `v1`/`v2` providers.
