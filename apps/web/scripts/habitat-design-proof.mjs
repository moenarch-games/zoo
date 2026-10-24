// #97 habitat design browser proof. Called by capture-park-view-proof.mjs in the same production
// preview and headless Chromium. On the Riverside zoo it turns the capybara habitat into a
// suitable one with the design panel and terrain tool (wading water, a deep pool, a raised tile,
// a shelter and a play boulder) and checks every capybara requirement turns met, with the scene
// showing zoo-core's water, height and furnishings. It then sees a bad edit (flooding the service
// gate) refused with nothing spent, makes an unsuitable edit (removing the boulder) and recovers
// from it, and paints a tile by touch on a phone-sized viewport.
import { enterNewPark } from "./park-library-proof.mjs"

const LAYOUT_PARTS = ["shelter", "enrichment", "ground_cover", "shallow_water", "deep_water"]

export async function runHabitatDesignAcceptance(context) {
  const { cdp, evaluate, assert, sleep, previewUrl, tile, clickButton, mouse, key, frame, screenshot } =
    context

  const message = () => evaluate(`document.querySelector('.message')?.textContent ?? ''`)
  const waitFor = async (description, expression, attempts = 100) => {
    for (let attempt = 0; attempt < attempts; attempt += 1) {
      if (await evaluate(expression)) return
      await sleep(100)
    }
    throw new Error(`#97 acceptance timed out waiting for ${description}: ${await message()}`)
  }
  const cashText = () => evaluate(`document.querySelector('.stat strong')?.textContent ?? ''`)
  /** The welfare breakdown the panel shows: part key → met. */
  const parts = () =>
    evaluate(`Object.fromEntries([...document.querySelectorAll('.welfare-parts li')].map((li) =>
      [li.dataset.part, li.classList.contains('met')]))`)
  const metAll = (keys) =>
    `(() => { const li = (k) => document.querySelector('.welfare-parts li[data-part="' + k + '"]');
      return ${JSON.stringify(keys)}.every((k) => li(k)?.classList.contains('met')) })()`
  const brush = async (value) => {
    const pressed = await evaluate(`(() => {
      const button = document.querySelector('[data-brush="${value}"]')
      if (!button) return false
      button.click()
      return true
    })()`)
    assert(pressed, `#97: brush ${value} missing`)
    await sleep(150)
  }
  const press = (point) => mouse("mousePressed", point, { button: "left", buttons: 1, clickCount: 1 })
  const release = async (point) => {
    await mouse("mouseReleased", point, { button: "left", buttons: 0, clickCount: 1 })
    await frame()
  }
  const tap = async (x, y) => {
    const point = await tile(x, y)
    await mouse("mouseMoved", point)
    await press(point)
    await release(point)
    await sleep(150)
  }
  const drag = async (from, to) => {
    const a = await tile(...from)
    const b = await tile(...to)
    await mouse("mouseMoved", a)
    await press(a)
    for (let step = 1; step <= 8; step += 1) {
      const t = step / 8
      await mouse("mouseMoved", { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t }, { button: "left", buttons: 1 })
    }
    await frame()
    await release(b)
    await sleep(150)
  }
  const expectMessage = async (text) =>
    waitFor(`the message "${text}"`, `document.querySelector('.message')?.textContent.includes(${JSON.stringify(text)}) ?? false`)
  const node = (id) => evaluate(`window.__zooParkView.staticNode(${JSON.stringify(id)})`)
  const settleCamera = async () => {
    let last = ""
    for (let attempt = 0; attempt < 60; attempt += 1) {
      const now = JSON.stringify(await evaluate("window.__zooParkView.camera()"))
      if (now === last) return
      last = now
      await sleep(150)
    }
  }
  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })
  await cdp.send("Page.navigate", { url: previewUrl })
  assert(
    (await enterNewPark({ evaluate, sleep }, { scenario: "living-starter" })) === "started",
    "#97: Riverside zoo did not start",
  )
  await waitFor("the park renderer", `Boolean(window.__zooParkView?.ready && window.__zooParkView.nodeCount() > 500)`, 200)
  assert(await clickButton("Ⅱ"), "#97: pause control missing")
  await key("Home")
  await settleCamera()

  // Inspect the capybara habitat through its fence-side corner tile.
  assert(await clickButton("Inspect"), "#97: inspect tool missing")
  await tap(8, 9)
  await waitFor("habitat #1's inspector", `document.querySelector('.window-title span')?.textContent === 'Habitat #1'`)
  await waitFor("the habitat design panel", `Boolean(document.querySelector('.habitat-design'))`)
  const before = await parts()
  for (const key of ["shelter", "enrichment", "shallow_water", "deep_water"]) {
    assert(before[key] === false, `#97: a plain grass habitat already meets ${key}`, before)
  }

  // --- Build a suitable habitat -------------------------------------------------------------
  assert(await clickButton("Design terrain"), "#97: design terrain control missing")
  await waitFor("the brushes", `Boolean(document.querySelector('.design-brushes'))`)
  await brush("surface:shallow_water")
  await drag([4, 5], [5, 5])
  await expectMessage("Paint 2 tiles shallow water")
  await brush("surface:deep_water")
  await tap(8, 5)
  await expectMessage("Paint 1 tile deep water")
  await brush("object:basic_shelter")
  await tap(8, 7)
  await expectMessage("Place Basic shelter at 8,7")
  await brush("object:play_boulder")
  await tap(4, 7)
  await expectMessage("Place Play boulder at 4,7")
  // A numeric height: one step raises the tile without cutting it off.
  assert(
    await evaluate(`(() => {
      const input = document.querySelector('input[aria-label="Tile height"]')
      if (!input) return false
      input.focus()
      const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set
      setter.call(input, '1')
      input.dispatchEvent(new Event('input', { bubbles: true }))
      return true
    })()`),
    "#97: tile height input missing",
  )
  await sleep(150)
  await tap(6, 6)
  await expectMessage("Set 1 tile to height 1")
  await waitFor("every capybara layout requirement met", metAll(LAYOUT_PARTS))

  // The scene shows exactly zoo-core's layout: sunken water, a raised tile, both furnishings.
  await waitFor("the raised tile in the scene", `(window.__zooParkView.staticNode("tile:6:6")?.geometry.size[1] ?? 0) > 0.1`)
  const pool = await node("tile:4:5")
  const deep = await node("tile:8:5")
  const grass = await node("tile:5:7")
  const raised = await node("tile:6:6")
  assert(pool && deep && grass && raised, "#97: habitat tiles missing from the scene")
  assert(pool.geometry.size[1] < grass.geometry.size[1], "#97: shallow water is not sunken", { pool, grass })
  assert(deep.geometry.size[1] < pool.geometry.size[1], "#97: deep water is not deeper", { deep, pool })
  assert(raised.geometry.size[1] > grass.geometry.size[1], "#97: the raised tile is flat", { raised, grass })
  assert(pool.color !== grass.color, "#97: water has the grass colour", { pool, grass })
  const objects = await evaluate(`[...document.querySelectorAll('.habitat-objects li')].map((li) => li.textContent)`)
  assert(objects.length === 2, "#97: the furnishing list does not show both objects", objects)
  const boulderId = Number(objects.find((text) => text.startsWith("Play boulder")).match(/#(\d+)/)[1])
  const shelterId = Number(objects.find((text) => text.startsWith("Basic shelter")).match(/#(\d+)/)[1])
  await waitFor("the shelter in the scene", `window.__zooParkView.staticNode("habitat:1:object:${shelterId}:roof") !== null`)
  await waitFor("the play boulder in the scene", `window.__zooParkView.staticNode("habitat:1:object:${boulderId}:boulder") !== null`)
  await screenshot("test-results/habitat-design-desktop.png")

  // --- A bad edit is refused whole -----------------------------------------------------------
  const cashBefore = await cashText()
  await brush("surface:deep_water")
  const gate = await tile(6, 9)
  await mouse("mouseMoved", gate)
  await frame()
  await waitFor("the refused preview", `document.querySelector('.design-brushes .zone-warning')?.textContent.includes('dry land') ?? false`)
  await tap(6, 9)
  await expectMessage("The service gate must open onto dry land")
  assert((await cashText()) === cashBefore, "#97: a refused edit spent money", { cashBefore, after: await cashText() })
  assert(await evaluate(metAll(LAYOUT_PARTS)), "#97: a refused edit changed the layout", await parts())
  assert((await node("tile:6:9")).geometry.size[1] === grass.geometry.size[1], "#97: the gate tile changed in the scene")

  // --- An unsuitable edit, then recovery -----------------------------------------------------
  assert(await clickButton(`Remove Play boulder #${boulderId}`), "#97: remove control missing")
  await expectMessage(`Play boulder #${boulderId} removed`)
  await waitFor("enrichment to become unmet", `document.querySelector('.welfare-parts li[data-part="enrichment"]')?.classList.contains('unmet') ?? false`)
  await waitFor("the removed boulder to leave the scene", `window.__zooParkView.staticNode("habitat:1:object:${boulderId}:boulder") === null`)
  await brush("object:play_boulder")
  await tap(4, 7)
  await expectMessage("Place Play boulder at 4,7")
  await waitFor("the recovered habitat", metAll(LAYOUT_PARTS))
  await screenshot("test-results/habitat-design-recovered.png")

  // --- Phone: a touch tap paints one tile ----------------------------------------------------
  await cdp.send("Emulation.setDeviceMetricsOverride", { width: 390, height: 844, deviceScaleFactor: 2, mobile: true })
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: true, maxTouchPoints: 5 })
  await sleep(600)
  await key("Home")
  await settleCamera()
  await brush("surface:sand")
  const sand = await tile(5, 8)
  const onCanvas = await evaluate(`document.elementFromPoint(${sand.x}, ${sand.y})?.tagName === 'CANVAS'`)
  assert(sand.visible && onCanvas, "#97: the phone tile is not on the park canvas", { sand, hit: await evaluate(`(() => { const e = document.elementFromPoint(${sand.x}, ${sand.y}); return e ? e.tagName + "." + e.className + " < " + (e.closest("[class]")?.parentElement?.className ?? "") : null })()`) })
  await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: sand.x, y: sand.y, id: 0 }] })
  await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] })
  await frame()
  await expectMessage("Paint 1 tile sand")
  await screenshot("test-results/habitat-design-phone.png")
  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })

  console.log(
    "Habitat design acceptance passed: built a suitable capybara habitat (wading water, deep pool, raised tile, shelter, play boulder) with every requirement met and matching scene tiles and furnishings, saw flooding the gate refused with nothing spent, removed the boulder (enrichment unmet) and recovered, and painted sand by touch on a phone.",
  )
}
