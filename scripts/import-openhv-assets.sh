#!/usr/bin/env bash
set -eu

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
OPENHV_ROOT="${OPENHV_ROOT:-${1:-$HOME/Desktop/dev/openhv}}"
DESTINATION="${OPENHV_DESTINATION:-${2:-${REPO_ROOT}/client/public/openhv}}"
SOURCE="${OPENHV_ROOT}/mods/hv/bits"

assets=(
  sprites/infantry/rifleman
  sprites/vehicles/miner
  sprites/vehicles/mbt
  sprites/vehicles/artillery
  sprites/buildings/base
  sprites/buildings/outpost
  sprites/buildings/extractor
  sprites/buildings/factory
  sprites/buildings/techcenter
  sprites/buildings/turret
  sprites/terrain/grass
  sprites/terrain/rock1
  sprites/terrain/rock2
  sprites/gold
  sprites/effects/smudges1
  sprites/effects/expsmall
  sprites/effects/explosn
  sprites/effects/explobig
  sprites/effects/smoke
  sprites/effects/sparks1
  sprites/effects/bullet1
)

if [[ ! -d "${SOURCE}" ]]; then
  printf 'OpenHV assets not found: %s\n' "${SOURCE}" >&2
  printf 'Set OPENHV_ROOT to the OpenHV checkout or installation directory.\n' >&2
  exit 1
fi

for asset in "${assets[@]}"; do
  if [[ ! -f "${SOURCE}/${asset}.png" || ! -f "${SOURCE}/${asset}.yaml" ]]; then
    printf 'Missing OpenHV asset or metadata: %s\n' "${asset}" >&2
    exit 1
  fi
  mkdir -p "${DESTINATION}/$(dirname "${asset}")"
  cp "${SOURCE}/${asset}.png" "${DESTINATION}/${asset}.png"
  cp "${SOURCE}/${asset}.yaml" "${DESTINATION}/${asset}.yaml"
done

printf 'Imported %d OpenHV sprite sheets into %s\n' "${#assets[@]}" "${DESTINATION}"
printf 'The OpenHV checkout was read only; generated files are ignored by git.\n'
