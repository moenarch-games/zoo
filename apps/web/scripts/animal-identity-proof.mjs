// #96 individual animal browser proof. Called by capture-park-view-proof.mjs in the same
// production preview and headless Chromium. On the populated Riverside zoo it picks two animals
// of one habitat on the canvas and sees two distinct individuals with zoo-core's name, activity
// and needs, renames one, locates it, finds it in the habitat roster, saves, reloads, continues
// and picks it again under its new name; then taps an animal on a phone-sized touch viewport.
import { enterNewPark } from "./park-library-proof.mjs"

const MENU = `[aria-label="Park menu"][role="dialog"]`
const START = `[aria-label="Zoo start"]`

export async function runAnimalIdentityAcceptance(context) {
  const { cdp, evaluate, assert, sleep, previewUrl, clickButton, mouse, frame, screenshot } =
    context

  const message = () => evaluate(`document.querySelector('.message')?.textContent ?? ''`)
  const title = () => evaluate(`document.querySelector('.window-title span')?.textContent ?? ''`)
  const waitFor = async (description, expression, attempts = 100) => {
    for (let attempt = 0; attempt < attempts; attempt += 1) {
      if (await evaluate(expression)) return
      await sleep(100)
    }
    throw new Error(
      `#96 acceptance timed out waiting for ${description}: ${await title()} | ${await message()}`,
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
  const clickAnimal = async (id) => {
    const point = await animalPoint(id)
    await mouse("mouseMoved", point)
    await mouse("mousePressed", point, { button: "left", buttons: 1, clickCount: 1 })
    await mouse("mouseReleased", point, { button: "left", buttons: 0, clickCount: 1 })
    await frame()
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

  // Two capybaras of habitat #1 are two different individuals, each with zoo-core's activity.
  const first = await clickAnimal(1)
  assert(/^Walking|^Resting/.test(first.activity), "#96: no authoritative activity", first)
  assert(
    first.needs.includes("Energy") && first.needs.includes("Food") && first.needs.includes("Social fit"),
    "#96: individual and shared needs missing",
    first,
  )
  const second = await clickAnimal(2)
  assert(second.id === 2 && second.title.endsWith("#2"), "#96: second pick did not select animal #2", second)
  assert(first.id !== second.id, "#96: two animals share a selection", { first, second })
  await screenshot("test-results/animal-inspector-desktop.png")

  // Rename through zoo-core; the name sticks and the roster shows it.
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
  assert(await clickButton("Locate"), "#96: locate control missing")
  assert((await message()).includes("Camera moved to Biscuit"), "#96: locate failed", await message())
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
  const resumed = await clickAnimal(2)
  assert(resumed.title === "Biscuit · #2", "#96: the name did not survive a reload", resumed)

  // Phone: a tap on an animal opens its inspector.
  await cdp.send("Emulation.setDeviceMetricsOverride", {
    width: 390,
    height: 844,
    deviceScaleFactor: 2,
    mobile: true,
  })
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: true, maxTouchPoints: 5 })
  await evaluate(`document.querySelector('.window-title button')?.click()`)
  await sleep(600)
  const tap = await animalPoint(3)
  await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: tap.x, y: tap.y, id: 0 }] })
  await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] })
  await frame()
  await waitFor("animal #3's inspector on the phone", `document.querySelector('.animal-card')?.dataset.animalId === '3'`)
  await screenshot("test-results/animal-inspector-phone.png")
  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })

  console.log(
    `Animal identity acceptance passed: picked animals #1 and #2 of one habitat as distinct individuals (${first.title}: ${first.activity}), renamed #2 to Biscuit, located it, found it in the roster, kept it through save → reload → continue, and tapped animal #3 on a phone.`,
  )
}
