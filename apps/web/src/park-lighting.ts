// Presentation of the simulation's daylight and weather (#118) through the shared renderer's
// frame environment. zoo-core decides the time of day, season and weather; this file only maps
// them to sky, sun and background colors. Night keeps a strong ambient floor so paths, animals,
// selections and construction previews stay readable, and both display settings switch the
// mapping off without touching the game.
import type { RendererEnvironment } from "@moritzbrantner/three-d-renderer"

import type { DisplaySettings } from "./ConditionsPanel"
import type { ConditionsState } from "./game-types"

type Rgb = [number, number, number]

function rgb(hex: string): Rgb {
  const value = Number.parseInt(hex.slice(1), 16)
  return [(value >> 16) & 255, (value >> 8) & 255, value & 255]
}

function hex([r, g, b]: Rgb): `#${string}` {
  const part = (value: number) =>
    Math.round(Math.min(255, Math.max(0, value)))
      .toString(16)
      .padStart(2, "0")
  return `#${part(r)}${part(g)}${part(b)}`
}

function mix(from: Rgb, to: Rgb, t: number): Rgb {
  return [0, 1, 2].map((i) => from[i]! + (to[i]! - from[i]!) * t) as Rgb
}

function tint(color: Rgb, by: Rgb): Rgb {
  return [0, 1, 2].map((i) => (color[i]! * by[i]!) / 255) as Rgb
}

const clamp01 = (value: number) => Math.min(1, Math.max(0, value))

/** The park view's daytime clear color; it must match the renderer's `background` option. */
export const DAY_BACKGROUND = "#a9d6e5"

// Daytime values are the renderer defaults, so a midday frame looks exactly as before.
const DAY = {
  sky: rgb("#ffffff"),
  ground: rgb("#334433"),
  skyIntensity: 1.7,
  sun: rgb("#ffffff"),
  sunIntensity: 2.2,
  background: rgb(DAY_BACKGROUND),
}
const NIGHT = {
  sky: rgb("#8ea3d9"),
  ground: rgb("#28303c"),
  // The ambient floor that keeps the park readable at night.
  skyIntensity: 1,
  sun: rgb("#aabcff"),
  sunIntensity: 0.4,
  background: rgb("#26304a"),
}
const TWILIGHT_SUN = rgb("#ffb27a")
const TWILIGHT_BACKGROUND = rgb("#e7a98c")
const HEAT_TINT = rgb("#fff0d2")
const HEAT_BACKGROUND = rgb("#e6d6b4")
// The renderer default sun direction; the moon and steady daylight shine from it.
const HIGH_LIGHT: [number, number, number] = [10, 18, 8]

/**
 * The frame environment for the park's light and weather, or `undefined` for the renderer's
 * default daylight when lighting is off and no weather is shown.
 */
export function parkEnvironment(
  conditions: ConditionsState,
  display: DisplaySettings,
): RendererEnvironment | undefined {
  const heat = display.weatherVisuals && conditions.weather === "heat" && conditions.effects_active
  if (!display.daylight && !heat) {
    return undefined
  }
  const daylight = display.daylight ? conditions.light.daylight_percent / 100 : 1
  // 0 at night, 1 from mid-morning: the share of daylight in the mix.
  const day = clamp01(daylight / 0.6)
  // Warm light near the horizon: strongest around sunrise and sunset.
  const low = display.daylight ? clamp01(1 - Math.abs(daylight - 0.2) / 0.25) : 0
  let sun = mix(mix(NIGHT.sun, DAY.sun, day), TWILIGHT_SUN, low * 0.8)
  let background = mix(mix(NIGHT.background, DAY.background, day), TWILIGHT_BACKGROUND, low * 0.5)
  let sky = mix(NIGHT.sky, DAY.sky, day)
  if (heat) {
    sun = tint(sun, HEAT_TINT)
    sky = tint(sky, HEAT_TINT)
    background = mix(background, HEAT_BACKGROUND, 0.45 * day)
  }
  // The sun crosses the sky from east to west by the simulation clock; the moon stays high.
  const arc = (conditions.light.sun_arc_permille / 1000) * Math.PI
  const direction: [number, number, number] =
    display.daylight && daylight > 0.2
      ? [Math.cos(arc) * 18, 6 + Math.sin(arc) * 14, 8]
      : HIGH_LIGHT
  return {
    background: hex(background),
    sky: {
      skyColor: hex(sky),
      groundColor: hex(mix(NIGHT.ground, DAY.ground, day)),
      intensity: NIGHT.skyIntensity + (DAY.skyIntensity - NIGHT.skyIntensity) * day,
    },
    sun: {
      direction,
      color: hex(sun),
      intensity: NIGHT.sunIntensity + (DAY.sunIntensity - NIGHT.sunIntensity) * day,
    },
  }
}
