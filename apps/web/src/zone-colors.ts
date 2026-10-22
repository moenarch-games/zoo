/** Presentation-only colours for keeper work zones, stable per zone id. */
const ZONE_COLORS = [
  "#3f8fd8",
  "#9b5fd0",
  "#2fae9a",
  "#e07ab8",
  "#6a7be0",
  "#c99a2e",
  "#4bb3d9",
  "#b06b4a",
] as const

export const NO_ZONE_COLOR = "#e0473a"

export function zoneColor(zoneId: number): `#${string}` {
  return ZONE_COLORS[(zoneId - 1) % ZONE_COLORS.length] ?? ZONE_COLORS[0]
}
