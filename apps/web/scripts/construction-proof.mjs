// #92 transactional construction browser proof. Called by capture-park-view-proof.mjs on a fresh
// park in the same production preview and headless Chromium. Every check drives real canvas
// pointer, keyboard and touch input; prices are read from Rust's preview tooltip, not hardcoded.
import { enterNewPark } from "./park-library-proof.mjs"

export async function runConstructionAcceptance(context) {
  const {
    cdp,
    evaluate,
    assert,
    sleep,
    previewUrl,
    tile,
    clickButton,
    mouse,
    key,
    frame,
    screenshot,
  } = context

  const waitFor = async (description, expression, attempts = 80) => {
    for (let attempt = 0; attempt < attempts; attempt += 1) {
      if (await evaluate(expression)) return
      await sleep(100)
    }
    throw new Error(`#92 acceptance timed out waiting for ${description}: ${await message()}`)
  }
  const message = () => evaluate(`document.querySelector('.message')?.textContent ?? ''`)
  const tooltip = () => evaluate(`document.querySelector('.park-tooltip')?.textContent ?? ''`)
  const cashCents = async () =>
    Math.round(
      Number(
        (await evaluate(`document.querySelector('.stat strong')?.textContent ?? ''`)).replace(
          /[^0-9.-]/g,
          "",
        ),
      ) * 100,
    )
  const tooltipCents = async () => {
    const headline = await evaluate(`document.querySelector(".park-tooltip b")?.textContent ?? ""`)
    const match = headline.match(/\$([0-9,]+)/)
    assert(match, "#92: preview tooltip shows no price", await tooltip())
    return Number(match[1].replaceAll(",", "")) * 100
  }
  const chord = async (value, modifiers) => {
    await cdp.send("Input.dispatchKeyEvent", {
      type: "keyDown",
      key: value,
      code: `Key${value.toUpperCase()}`,
      modifiers,
    })
    await cdp.send("Input.dispatchKeyEvent", {
      type: "keyUp",
      key: value,
      code: `Key${value.toUpperCase()}`,
      modifiers,
    })
    await frame()
  }
  const CTRL = 2
  const SHIFT = 8
  const press = (point) =>
    mouse("mousePressed", point, { button: "left", buttons: 1, clickCount: 1 })
  const release = async (point) => {
    await mouse("mouseReleased", point, { button: "left", buttons: 0, clickCount: 1 })
    await frame()
  }
  const moveHeld = async (from, to) => {
    for (let step = 1; step <= 8; step += 1) {
      const t = step / 8
      await mouse(
        "mouseMoved",
        { x: from.x + (to.x - from.x) * t, y: from.y + (to.y - from.y) * t },
        { button: "left", buttons: 1 },
      )
    }
    await frame()
  }
  const click = async (point) => {
    await mouse("mouseMoved", point)
    await press(point)
    await release(point)
  }

  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })
  await cdp.send("Page.navigate", { url: previewUrl })
  assert((await enterNewPark({ evaluate, sleep })) === "started", "#92: start screen did not start a new park")
  await waitFor(
    "the shared 3D park renderer",
    `Boolean(
    window.__zooParkView?.ready && window.__zooParkView.nodeCount() > 500 &&
    document.querySelector('.park-view[data-renderer="ready"] canvas.park-canvas'))`,
  )
  assert(await clickButton("Ⅱ"), "#92: pause control missing")
  const startCash = await cashCents()
  assert(
    await evaluate(`document.querySelector('button[aria-label="Undo"]')?.disabled === true`),
    "#92: undo is enabled on a fresh park",
  )

  // Path painting: the held stroke previews its exact price; release builds that price.
  assert(await clickButton("Path"), "#92: path tool missing")
  const from = await tile(5, 7)
  const to = await tile(9, 7)
  await mouse("mouseMoved", from)
  await press(from)
  await moveHeld(from, to)
  await waitFor(
    "the path stroke preview",
    `document.querySelector('.park-tooltip')?.textContent.includes('5 new public path tiles') ?? false`,
  )
  const quoted = await tooltipCents()
  await screenshot("test-results/construction-path-preview.png")
  await release(to)
  assert(
    (await message()).includes("Path built · 5 new public path tiles"),
    "#92: stroke did not build as one transaction",
    await message(),
  )
  const builtCash = await cashCents()
  assert(startCash - builtCash === quoted, "#92: committed path cost differs from its preview", {
    startCash,
    builtCash,
    quoted,
  })

  // Escape cancels a held stroke: nothing is built or spent.
  const cancelFrom = await tile(10, 7)
  await mouse("mouseMoved", cancelFrom)
  await press(cancelFrom)
  await moveHeld(cancelFrom, await tile(13, 7))
  await key("Escape")
  await release(await tile(13, 7))
  assert(
    (await message()).includes("cancelled"),
    "#92: Escape did not cancel the stroke",
    await message(),
  )
  assert((await cashCents()) === builtCash, "#92: a cancelled stroke spent money")

  // Invalid ghost: a stroke through the depot is refused whole and explains why.
  const blockedFrom = await tile(3, 5)
  await mouse("mouseMoved", blockedFrom)
  await press(blockedFrom)
  await moveHeld(blockedFrom, await tile(3, 6))
  await moveHeld(await tile(3, 6), await tile(1, 6))
  await waitFor(
    "the blocked stroke preview",
    `document.querySelector('.park-tooltip .invalid')?.textContent.includes('depot blocks') ?? false`,
  )
  await screenshot("test-results/construction-blocked-stroke.png")
  await release(await tile(1, 6))
  assert(
    (await message()).includes("depot blocks"),
    "#92: blocked stroke was not explained",
    await message(),
  )
  assert((await cashCents()) === builtCash, "#92: a blocked stroke spent money")

  // Undo → redo → undo → redo through buttons and shortcuts never mints money.
  for (let cycle = 0; cycle < 2; cycle += 1) {
    if (cycle === 0) assert(await clickButton("Undo"), "#92: undo button missing")
    else await chord("z", CTRL)
    assert(
      (await message()).startsWith("Undid 5 path tiles"),
      "#92: undo did not reverse the stroke",
      await message(),
    )
    assert((await cashCents()) === startCash, "#92: undo did not refund exactly", {
      cash: await cashCents(),
      startCash,
    })
    if (cycle === 0) await chord("z", CTRL | SHIFT)
    else assert(await clickButton("Redo"), "#92: redo button missing")
    assert(
      (await message()).startsWith("Redid 5 path tiles"),
      "#92: redo did not rebuild the stroke",
      await message(),
    )
    assert((await cashCents()) === builtCash, "#92: redo did not charge exactly", {
      cash: await cashCents(),
      builtCash,
    })
  }

  // Rotated numeric placement: rotate the typed 4×3 size and tap a corner.
  assert(await clickButton("Draw habitat"), "#92: habitat tool missing")
  assert(await clickButton("Rotate habitat"), "#92: rotate control missing")
  await mouse("mouseMoved", await tile(5, 8))
  await waitFor(
    "the rotated footprint preview",
    `document.querySelector('.park-tooltip')?.textContent.includes('3×4') ?? false`,
  )
  await click(await tile(5, 8))
  assert(
    (await message()).includes("Habitat #1 fenced · 3×4"),
    "#92: rotated numeric placement failed",
    await message(),
  )

  // Selection still resolves the right habitat after the camera turns.
  assert(await clickButton("Inspect"), "#92: inspect tool missing")
  await key("e", 3)
  await click(await tile(6, 10))
  await waitFor(
    "the habitat inspector after rotation",
    `document.querySelector('.window-title span')?.textContent === 'Habitat #1'`,
  )
  await key("Home")
  await chord("z", CTRL)
  assert(
    (await message()).startsWith("Undid habitat #1"),
    "#92: habitat undo failed",
    await message(),
  )
  assert((await cashCents()) === builtCash, "#92: habitat undo did not refund exactly")
  await chord("y", CTRL)
  assert(
    (await message()).startsWith("Redid habitat #1"),
    "#92: habitat redo failed",
    await message(),
  )

  // Demolition previews its consequences, and an empty-construction demolition is undoable.
  assert(await clickButton("Demolish"), "#92: demolish tool missing")
  await mouse("mouseMoved", await tile(9, 7))
  await waitFor(
    "the demolition preview",
    `document.querySelector('.park-tooltip')?.textContent.includes('undoable') ?? false`,
  )
  await click(await tile(9, 7))
  assert((await message()).includes("Path removed"), "#92: demolition failed", await message())
  assert(await clickButton("Undo"), "#92: undo after demolition missing")
  assert(
    (await message()).startsWith("Undid path tile removal"),
    "#92: demolition undo failed",
    await message(),
  )

  // --- Phone-sized touch -------------------------------------------------------------------------
  await cdp.send("Emulation.setDeviceMetricsOverride", {
    width: 390,
    height: 844,
    deviceScaleFactor: 2,
    mobile: true,
  })
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: true, maxTouchPoints: 5 })
  await evaluate(`document.querySelector(".window-title button")?.click()`)
  // The training checklist opened at desktop size; collapse it so the map takes touches.
  await evaluate(
    `(() => { const card = document.querySelector('details[aria-label="Park guide"]'); if (card) card.open = false })()`,
  )
  await key("Home")
  const touch = (type, points) =>
    cdp.send("Input.dispatchTouchEvent", {
      type,
      touchPoints: points.map(([x, y], id) => ({ x, y, id })),
    })
  const swipe = async (a, b, end = "touchEnd") => {
    await touch("touchStart", [[a.x, a.y]])
    for (let step = 1; step <= 8; step += 1) {
      const t = step / 8
      await touch("touchMove", [[a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t]])
    }
    await touch(end, [])
    await frame()
  }
  assert(await clickButton("Path"), "#92: path tool missing on phone")
  const phoneCash = await cashCents()
  await swipe(await tile(10, 7), await tile(13, 7))
  assert(
    (await message()).includes("Path built · 4 new public path tiles"),
    "#92: touch stroke did not build",
    await message(),
  )
  const phoneBuilt = await cashCents()
  // A cancelled touch (the OS takes the pointer) builds nothing.
  await swipe(await tile(14, 7), await tile(16, 7), "touchCancel")
  assert((await cashCents()) === phoneBuilt, "#92: a cancelled touch stroke spent money")
  // A second finger turns the stroke into a camera gesture and cancels it.
  const a = await tile(14, 7)
  const b = await tile(16, 7)
  await touch("touchStart", [[a.x, a.y]])
  for (let step = 1; step <= 4; step += 1)
    await touch("touchMove", [[a.x + ((b.x - a.x) * step) / 4, a.y + ((b.y - a.y) * step) / 4]])
  await touch("touchStart", [
    [b.x, b.y],
    [b.x + 80, b.y],
  ])
  await touch("touchEnd", [])
  await frame()
  assert((await cashCents()) === phoneBuilt, "#92: a multitouch-cancelled stroke spent money")
  await key("Home")
  assert(await clickButton("Undo"), "#92: undo missing on phone")
  assert((await cashCents()) === phoneCash, "#92: phone undo did not refund exactly")
  assert(await clickButton("Redo"), "#92: redo missing on phone")
  assert((await cashCents()) === phoneBuilt, "#92: phone redo did not charge exactly")
  await screenshot("test-results/construction-phone.png")
  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })

  console.log(
    "#92 construction dogfood passed: previewed path price matches its commit, Escape/touch-cancel/multitouch cancel spend nothing, blocked strokes refuse whole, undo/redo by button and shortcut reconcile cash exactly, rotated numeric placement, selection after camera rotation, undoable demolition (desktop + phone).",
  )
}
