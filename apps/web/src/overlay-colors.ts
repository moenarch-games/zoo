import type { OverlayLevel } from "./game-types"

/** Management overlay grades (#94), shared by the park scene and the HUD legend. */
export const OVERLAY_COLORS: Record<OverlayLevel, `#${string}`> = {
  good: "#3fae6a",
  warn: "#e3b23c",
  bad: "#d9483b",
}
