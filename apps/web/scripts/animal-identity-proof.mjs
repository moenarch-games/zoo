// #96 individual animals and #141 watch/follow browser proof. Called by
// capture-park-view-proof.mjs in the same production preview and headless Chromium. On the
// populated Riverside zoo it picks an animal on the canvas, which watches it first: a compact
// label with zoo-core's name and activity, no inspector, and the zoo-scene camera easing onto
// and following the individual through a full orbit at both tilt bounds without changing the
// authoritative park. Escape leaves follow; a second animal of the same habitat is a distinct
// individual; Details opens its inspector (needs, rename, locate, roster); the name survives
// save → reload → continue; the camera tracks a walking animal until a pan ends follow; and on a
// phone-sized touch viewport a tap watches an animal and the label's stop button ends it.
import { enterNewPark } from "./park-library-proof.mjs"

const MENU = `[aria-label="Park menu"][role="dialog"]`
const START = `[aria-label="Zoo start"]`

export async function runAnimalIdentityAcceptance(context) {
  const { cdp, evaluate, assert, sleep, previewUrl, clickButton, mouse, key, frame, screenshot } =
    context

  const message = () => evaluate(`document.querySelector('.message')?.textContent ?? ''`)
  const title = () => evaluate(`document.querySelector('.window-title span')?.textContent ?? ''`)
  const waitFor = async (description, expression, attempts = 100) => {
    for (let attempt = 0; attempt < attempts; attempt += 1) {
      if (await evaluate(expression)) return
      await sleep(100)
    }
    throw new Error(
      `#96/#141 acceptance timed out waiting for ${description}: ${await title()} | ${await message()}`,
    )
  }
  const press = async (scope, label) => {
    const pressed = await evaluate(`(() => {
      const button = [...document.querySelectorAll(${JSON.stringify(`${scope} button`)})].find((candidate) =>
        (candidate.getAttribute('aria-label') ?? candidate.textContent ?? '').startsWith(${JSON.stringify(label)}))
      if (!button || button.disabled) return false
      button.click()
      return true
    })()`)
    assert(pressed, `#96: ${label} control missing`)
    await sleep(150)
  }
  const ready = () =>
    waitFor(
      "the park renderer",
      `Boolean(window.__zooParkView?.ready && window.__zooParkView.nodeCount() > 500)`,
      200,
    )
  /** The complete authoritative park (zoo-save document): what watching must never change. */
  const parkState = () => evaluate(`window.__zooSimulation.stateJson()`)
  /** zoo-core's own record of animal `id`. */
  const coreAnimal = (id) =>
    evaluate(`JSON.parse(window.__zooSimulation.snapshotJson()).animals.find((a) => a.id === ${id}) ?? null`)
  const following = () => evaluate(`window.__zooParkView.followAnimalId()`)
  /** The compact watch label: the watched ID, name and activity text. */
  const watchLabel = () =>
    evaluate(`(() => {
      const label = document.querySelector('[data-animal-watch]')
      if (!label) return null
      return {
        id: Number(label.dataset.animalWatch),
        name: label.querySelector('b')?.textContent ?? '',
        activity: label.querySelector('[data-testid="watch-activity"]')?.textContent ?? '',
      }
    })()`)
  /** The card of the inspected animal: its stable ID, title, activity and needs. */
  const inspected = () =>
    evaluate(`(() => {
      const card = document.querySelector('.animal-card')
      if (!card) return null
      return {
        id: Number(card.dataset.animalId),
        title: document.querySelector('.window-title span')?.textContent ?? '',
        activity: card.querySelector('[data-testid="animal-activity"]')?.textContent ?? '',
        needs: [...card.querySelectorAll('.need-row span')].map((label) => label.textContent),
      }
    })()`)
  const animalPoint = async (id) => {
    const point = await evaluate(`window.__zooParkView.animalPoint(${id})`)
    assert(point?.visible, `#96: animal #${id} is not on screen`, point)
    return point
  }
  const settled = (id) =>
    waitFor(
      `the camera to settle on animal #${id}`,
      `(() => {
        const point = window.__zooParkView.animalPoint(${id})
        const rect = document.querySelector('.park-canvas').getBoundingClientRect()
        return point?.visible && Math.hypot(point.x - rect.left - rect.width / 2, point.y - rect.top - rect.height / 2) < 24
      })()`,
    )
  /**
   * Zooms in on and orbits around animal `id` until it stands clear of every other animal on screen and of the HUD, so a
   * click on it cannot land on a neighbour (where animals stand depends on how long the park ran).
   */
  const isolate = async (id) => {
    for (let turn = 0; turn < 24; turn += 1) {
      const clear = await evaluate(`(() => {
        const own = window.__zooParkView.animalPoint(${id})
        if (!own?.visible || !document.elementFromPoint(own.x, own.y)?.classList.contains('park-canvas')) return false
        return JSON.parse(window.__zooSimulation.snapshotJson()).animals.every((other) => {
          const point = other.id === ${id} ? null : window.__zooParkView.animalPoint(other.id)
          return !point?.visible || Math.hypot(point.x - own.x, point.y - own.y) > 32
        })
      })()`)
      if (clear) {
        // …and has stopped moving on screen (presented positions ease after a pause).
        const a = await evaluate(`window.__zooParkView.animalPoint(${id})`)
        await sleep(200)
        const b = await evaluate(`window.__zooParkView.animalPoint(${id})`)
        if (Math.hypot(a.x - b.x, a.y - b.y) < 2) return
        continue
      }
      const own = await evaluate(`window.__zooParkView.animalPoint(${id})`)
      if (turn < 10 && own?.visible) {
        await cdp.send("Input.dispatchMouseEvent", { type: "mouseWheel", x: own.x, y: own.y, deltaX: 0, deltaY: -300 })
        await sleep(150)
      } else {
        await key("e")
      }
    }
    throw new Error(`#96: animal #${id} never stands clear of the others: ${await evaluate(`JSON.stringify(JSON.parse(window.__zooSimulation.snapshotJson()).animals.map((a) => [a.id, window.__zooParkView.animalPoint(a.id)]))`)}`)
  }
  /** Picks animal `id` on the canvas: it is watched, not inspected. */
  const watchAnimal = async (id) => {
    await isolate(id)
    const point = await animalPoint(id)
    await mouse("mouseMoved", point)
    await mouse("mousePressed", point, { button: "left", buttons: 1, clickCount: 1 })
    await mouse("mouseReleased", point, { button: "left", buttons: 0, clickCount: 1 })
    await frame()
    await waitFor(`animal #${id}'s watch label`, `document.querySelector('[data-animal-watch]')?.dataset.animalWatch === '${id}'`)
    await waitFor(`the camera to follow animal #${id}`, `window.__zooParkView.followAnimalId() === ${id}`)
    const label = await watchLabel()
    const core = await coreAnimal(id)
    assert(
      label.name === core.name && label.activity.toLowerCase() === core.activity,
      "#141: the watch label is not zoo-core's name and activity",
      { label, core },
    )
    assert(!(await evaluate(`Boolean(document.querySelector('.animal-card'))`)), "#141: watching opened the inspector")
    return label
  }
  const details = async (id) => {
    await evaluate(`document.querySelector('[data-action="animal-details"]').click()`)
    await waitFor(`animal #${id}'s inspector`, `document.querySelector('.animal-card')?.dataset.animalId === '${id}'`)
    return inspected()
  }
  const startRiverside = async () => {
    assert(
      (await enterNewPark({ evaluate, sleep }, { scenario: "living-starter" })) === "started",
      "#96: Riverside zoo did not start",
    )
    await ready()
    assert(await clickButton("Ⅱ"), "#96: pause control missing")
    await sleep(600)
  }

  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })
  await cdp.send("Page.navigate", { url: previewUrl })
  await startRiverside()
  assert(await clickButton("Inspect"), "#96: inspect tool missing")

  // Watch first: picking capybara #1 shows its compact label and the camera eases onto it.
  const before = await parkState()
  const overview = await evaluate(`window.__zooParkView.camera()`)
  const first = await watchAnimal(1)
  await settled(1)
  await sleep(800)
  const close = await evaluate(`window.__zooParkView.camera()`)
  assert(close.zoom >= overview.zoom * 1.3, "#141: watching did not bring the camera close", { overview, close })
  await screenshot("test-results/animal-watch-desktop.png")

  // Orbit all the way round at both tilt bounds: the camera keeps following the same animal.
  for (const tilt of ["r", "f"]) {
    await key(tilt, 12)
    for (let turn = 0; turn < 24; turn += 1) {
      await key("e")
    }
    await settled(1)
    assert((await following()) === 1, `#141: orbiting at the ${tilt === "r" ? "top" : "bottom"} tilt bound ended follow`)
  }
  const tilted = await evaluate(`window.__zooParkView.camera()`)
  assert(tilted.pitchDegrees <= 23, "#141: the orbit did not reach the low tilt bound", tilted)
  assert((await parkState()) === before, "#141: watching and orbiting changed the authoritative park")

  // Escape leaves follow and the camera stays where it is; Home returns to the park overview.
  await key("Escape")
  await waitFor("Escape to end follow", `window.__zooParkView.followAnimalId() === null && !document.querySelector('[data-animal-watch]')`)
  await key("Home")
  await sleep(300)

  // A second capybara of the same habitat is a distinct individual.
  const second = await watchAnimal(2)
  assert(second.id === 2 && second.id !== first.id, "#96: the second pick did not watch animal #2", { first, second })
  const card = await details(2)
  assert(card.title.endsWith("#2"), "#96: Details did not inspect animal #2", card)
  assert(/^Walking|^Resting/.test(card.activity), "#96: no authoritative activity", card)
  assert(
    card.needs.includes("Energy") && card.needs.includes("Food") && card.needs.includes("Social fit"),
    "#96: individual and shared needs missing",
    card,
  )
  assert((await watchLabel())?.id === 2, "#141: opening the inspector dropped the watch label")
  await screenshot("test-results/animal-inspector-desktop.png")

  // Rename through zoo-core; the name sticks in the inspector, the watch label and the roster.
  assert(
    await evaluate(`(() => {
      const input = document.querySelector('input[aria-label="Animal name"]')
      if (!input) return false
      const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set
      setter.call(input, 'Biscuit')
      input.dispatchEvent(new Event('input', { bubbles: true }))
      return true
    })()`),
    "#96: animal name field missing",
  )
  await sleep(100)
  assert(await clickButton("Rename"), "#96: rename control missing")
  await waitFor("the rename", `document.querySelector('.window-title span')?.textContent === 'Biscuit · #2'`)
  assert((await message()).includes("now called Biscuit"), "#96: rename not confirmed", await message())
  assert((await watchLabel())?.name === "Biscuit", "#141: the watch label kept the old name")
  assert(await clickButton("Locate"), "#96: locate control missing")
  assert((await message()).includes("Camera moved to Biscuit"), "#96: locate failed", await message())
  assert((await following()) === 2, "#141: Locate does not watch the animal")
  assert(await clickButton("Show habitat"), "#96: show habitat control missing")
  await waitFor("the habitat roster", `[...document.querySelectorAll('.animal-roster button')].some((b) => b.textContent.startsWith('Biscuit'))`)
  const rosterSize = await evaluate(`document.querySelectorAll('.animal-roster li').length`)
  assert(rosterSize === 4, "#96: the roster does not list the habitat's four capybaras", rosterSize)
  await screenshot("test-results/animal-roster-desktop.png")

  // Save, reload, continue: the renamed individual keeps its ID and name.
  await press(".speed-controls", "Park menu")
  await press(MENU, "Save")
  await waitFor("the save", `document.querySelector('.library-message')?.textContent.startsWith('Saved “') ?? false`)
  await press(MENU, "Back to park")
  await cdp.send("Page.navigate", { url: previewUrl })
  await waitFor("the start screen", `Boolean(document.querySelector('${START} button'))`, 200)
  await press(START, "Continue")
  await ready()
  assert(await clickButton("Ⅱ"), "#96: pause control missing after reload")
  await sleep(600)
  assert(await clickButton("Inspect"), "#96: inspect tool missing after reload")
  const resumed = await watchAnimal(2)
  assert(resumed.name === "Biscuit", "#96: the name did not survive a reload", resumed)
  assert((await details(2)).title === "Biscuit · #2", "#96: the inspector lost the name after a reload")
  await evaluate(`document.querySelector('.window-title button')?.click()`)
  await key("Escape")
  await key("Home")

  // Running: the camera tracks a walking animal across simulated minutes; a pan ends follow.
  assert(await clickButton("1×"), "#141: play control missing")
  await waitFor(
    "a walking animal",
    `JSON.parse(window.__zooSimulation.snapshotJson()).animals.some((a) => a.activity === 'walking' && window.__zooParkView.animalPoint(a.id)?.visible)`,
    200,
  )
  const walker = await evaluate(
    `JSON.parse(window.__zooSimulation.snapshotJson()).animals.find((a) => a.activity === 'walking' && window.__zooParkView.animalPoint(a.id)?.visible).id`,
  )
  // Picked while paused so the click cannot miss a moving animal, then followed while it walks.
  assert(await clickButton("Ⅱ"), "#141: pause control missing")
  await watchAnimal(walker)
  assert(await clickButton("1×"), "#141: play control missing")
  await settled(walker)
  const startTarget = (await evaluate(`window.__zooParkView.camera()`)).target
  // With no camera input at all, the target travels only because it follows the walking animal.
  await waitFor(
    `the camera to travel with animal #${walker}`,
    `(() => { const t = window.__zooParkView.camera().target; return Math.hypot(t[0] - ${startTarget[0]}, t[2] - ${startTarget[2]}) > 0.5 })()`,
    400,
  )
  await settled(walker)
  assert((await following()) === walker, "#141: follow switched animals while running")
  const canvas = await evaluate(`(() => { const r = document.querySelector('.park-canvas').getBoundingClientRect(); return { x: r.left + r.width * 0.3, y: r.top + r.height * 0.6 } })()`)
  await mouse("mouseMoved", canvas)
  await mouse("mousePressed", canvas, { button: "left", buttons: 1, clickCount: 1 })
  for (let step = 1; step <= 8; step += 1) {
    await mouse("mouseMoved", { x: canvas.x + step * 12, y: canvas.y }, { button: "left", buttons: 1 })
  }
  await mouse("mouseReleased", { x: canvas.x + 96, y: canvas.y }, { button: "left", buttons: 0, clickCount: 1 })
  await waitFor("a pan to end follow", `window.__zooParkView.followAnimalId() === null && !document.querySelector('[data-animal-watch]')`)
  assert(await clickButton("Ⅱ"), "#141: pause control missing")
  await sleep(300)

  // Phone: a tap on an animal watches it; the label's stop button ends follow by touch.
  await cdp.send("Emulation.setDeviceMetricsOverride", {
    width: 390,
    height: 844,
    deviceScaleFactor: 2,
    mobile: true,
  })
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: true, maxTouchPoints: 5 })
  await key("Home")
  // Fold the scenario goals so they do not cover the habitat on the phone.
  await evaluate(`document.querySelectorAll('details[open] > summary').forEach((summary) => summary.click())`)
  await sleep(600)
  // The first animal no panel covers (where the animals stand depends on how long the park ran).
  const tapped = await evaluate(`[3, 4, 1, 5, 6, 7].find((id) => {
    const point = window.__zooParkView.animalPoint(id)
    return point?.visible && document.elementFromPoint(point.x, point.y)?.classList.contains('park-canvas')
  }) ?? null`)
  assert(tapped !== null, "#96: no animal is uncovered on the phone")
  await isolate(tapped)
  const tap = await animalPoint(tapped)
  const tapState = await parkState()
  await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: tap.x, y: tap.y, id: 0 }] })
  await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] })
  await frame()
  await waitFor(`animal #${tapped}'s watch label on the phone`, `document.querySelector('[data-animal-watch]')?.dataset.animalWatch === '${tapped}'`)
  await settled(tapped)
  assert(!(await evaluate(`Boolean(document.querySelector('.animal-card'))`)), "#141: a phone tap opened the inspector")
  await screenshot("test-results/animal-watch-phone.png")
  const stop = await evaluate(`(() => {
    const r = document.querySelector('[data-action="release-follow"]')?.getBoundingClientRect()
    return r && r.width >= 44 && r.height >= 44 ? { x: r.left + r.width / 2, y: r.top + r.height / 2 } : null
  })()`)
  assert(stop, "#141: no touch-sized stop-following control on the phone")
  await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: stop.x, y: stop.y, id: 0 }] })
  await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] })
  await waitFor("the stop control to end follow", `window.__zooParkView.followAnimalId() === null && !document.querySelector('[data-animal-watch]')`)
  assert((await parkState()) === tapState, "#141: watching on the phone changed the authoritative park")
  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })

  console.log(
    `Animal identity and watch acceptance passed: watching ${first.name} (#1, ${first.activity}) followed it through a full orbit at both tilt bounds without changing the park, Escape released it, animal #2 was a distinct individual renamed to Biscuit through its inspector, kept through save → reload → continue, the camera tracked walking animal #${walker} until a pan, and a phone tap watched animal #${tapped} until its stop control was tapped.`,
  )
}
