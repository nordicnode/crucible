# OpenHV asset bridge

Crucible can use a small, local-only selection of sprite sheets from the
[OpenHV](https://github.com/OpenHV/OpenHV) project. The importer copies the
selected PNG files and their per-file YAML metadata from an existing OpenHV
checkout into `client/public/openhv/`; that generated directory is ignored and
must not be committed.

Run the importer from the repository root:

```bash
bash scripts/import-openhv-assets.sh
```

Use `OPENHV_ROOT=/path/to/OpenHV` when the checkout is not at the default local
path. The renderer keeps its deterministic procedural sprites as a fallback
when the local files have not been imported or are still loading.

## Attribution

OpenHV source code is GPLv3. The selected content assets are not all under one
license; the metadata copied beside each local PNG is authoritative. The
selected sprite sheets currently identify these authors and licenses:

- **Daniel Cook** — original Hard Vacuum / Lost Garden artwork,
  [CC BY 3.0 US](https://creativecommons.org/licenses/by/3.0/us/).
- **Pawel Dzierzanowski** — OpenHV artwork and contributions,
  [CC BY 3.0 US](https://creativecommons.org/licenses/by/3.0/us/) or
  [CC BY-SA 4.0](https://creativecommons.org/licenses/by-sa/4.0/), as stated by
  each asset's YAML file.
- **SiegeSpud** — OpenHV artwork and contributions,
  [CC BY-SA 4.0](https://creativecommons.org/licenses/by-sa/4.0/), as stated by
  each asset's YAML file.

The source repository describes the broader sprite folder and license split in
`mods/hv/bits/sprites/README.md`. Keep that attribution and the applicable
Creative Commons license notices with any distributed build that includes the
imported assets. This bridge intentionally leaves the OpenHV checkout
untouched.
