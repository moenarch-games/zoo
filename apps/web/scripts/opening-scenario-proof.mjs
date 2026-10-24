// #93 browser acceptance: a first-time player walks the guided opening scenario to an operating
// zoo, on desktop with the mouse and on a phone with touch, then saves, reloads and resumes it.
//
// Called by capture-park-view-proof.mjs against the same production preview and real headless
// Chromium. Every step is taken the way a player would: the guide's own pointer button picks the
// tool or opens the inspector it names, and the park is built by real canvas picking. Progress
// is read from the guide the Rust park state renders; no command is issued from the script.
// Along the way the walk takes a wrong path (a stub cut off from the entrance), adopts too small
// a group and skips and restarts the guide, and checks each produces a useful recovery hint.

import { startPickedScenario } from "./park-library-proof.mjs"

const START = `[aria-label="Zoo start"]`
const MENU = `[aria-label="Park menu"][role="dialog"]`
const GUIDE = `details[aria-label="Park guide"]`

export async function runOpeningScenarioAcceptance(context) {
  const { cdp, evaluate, assert, sleep, previewUrl, screenshot, clickButton, frame } = context
  const waitFor = async (description, expression, attempts = 150) => {
    for (let attempt = 0; attempt < attempts; attempt += 1) {
      if (await evaluate(expression)) return
      await sleep(100)
    }
    const state = await evaluate(`({
      next: document.querySelector('${GUIDE} li.next')?.dataset.objective ?? null,
      problem: document.querySelector('${GUIDE} .tutorial-problem')?.textContent ?? null,
      message: document.querySelector('.message')?.textContent ?? null,
    })`)
    throw new Error(`#93 acceptance timed out waiting for ${description}: ${JSON.stringify(state)}`)
  }
  const project = (x, y) => evaluate(`window.__zooParkView.projectWorld([${x + 0.5}, 0, ${y + 0.5}])`)
  const message = () => evaluate(`document.querySelector('.message')?.textContent ?? ''`)
  const next = () => evaluate(`document.querySelector('${GUIDE} li.next')?.dataset.objective ?? null`)
  const problem = () => evaluate(`document.querySelector('${GUIDE} .tutorial-problem')?.textContent ?? ''`)
  const hint = () => evaluate(`document.querySelector('${GUIDE} .tutorial-hint')?.textContent ?? ''`)
  const setGuideOpen = (open) =>
    evaluate(`(() => { const card = document.querySelector('${GUIDE}'); if (card) card.open = ${open} })()`)
  const closeInspector = () =>
    evaluate(`(() => {
      const button = document.querySelector('.side-panel .window-title button')
      button?.click()
      return Boolean(button)
    })()`)

  /** Clicks the guide's contextual pointer, which must name `label`. */
  const followPointer = async (label, phone) => {
    await setGuideOpen(true)
    await frame()
    const pressed = await evaluate(`(() => {
      const button = [...document.querySelectorAll('${GUIDE} .tutorial-actions button')].find((candidate) =>
        candidate.textContent.startsWith(${JSON.stringify(label)}))
      button?.click()
      return Boolean(button)
    })()`)
    assert(pressed, `#93: the guide does not point at ${label}`, {
      next: await next(),
      buttons: await evaluate(`[...document.querySelectorAll('${GUIDE} .tutorial-actions button')].map((b) => b.textContent)`),
    })
    await frame()
    // On a phone the player folds the guide away again before touching the map.
    if (phone) await setGuideOpen(false)
  }

  /** The tile is under the canvas, not under the guide or another HUD control. */
  const reachable = async (label, point) => {
    assert(point.visible, `${label}: target is off screen`, point)
    assert(
      await evaluate(`document.elementFromPoint(${point.x}, ${point.y})?.classList.contains('park-canvas') ?? false`),
      `${label}: the HUD covers the target`,
      await evaluate(`document.elementFromPoint(${point.x}, ${point.y})?.outerHTML.slice(0, 200) ?? null`),
    )
  }

  // On a phone the expanded guide shares the screen with the map; the player folds it to one
  // line (its default there) before touching the park.
  const foldGuide = async () => {
    await setGuideOpen(false)
    await frame()
  }

  const pointerInput = (phone) => {
    const touch = (type, points) =>
      cdp.send("Input.dispatchTouchEvent", { type, touchPoints: points.map(([x, y], id) => ({ x, y, id })) })
    const mouse = (type, point, extra = {}) =>
      cdp.send("Input.dispatchMouseEvent", { type, x: point.x, y: point.y, button: "none", ...extra })
    return {
      async drag(label, from, to) {
        // Drawing needs the whole map: fold the guide and close any inspector panel.
        if (phone) {
          await foldGuide()
          await closeInspector()
          await frame()
        }
        const a = await project(from.x, from.y)
        const b = await project(to.x, to.y)
        await reachable(`${label} start`, a)
        await reachable(`${label} end`, b)
        if (phone) {
          await touch("touchStart", [[a.x, a.y]])
          for (let step = 1; step <= 10; step += 1) {
            const t = step / 10
            await touch("touchMove", [[a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t]])
          }
          await touch("touchEnd", [])
        } else {
          await mouse("mouseMoved", a)
          await mouse("mousePressed", a, { button: "left", buttons: 1, clickCount: 1 })
          for (let step = 1; step <= 10; step += 1) {
            const t = step / 10
            await mouse("mouseMoved", { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t }, { button: "left", buttons: 1 })
          }
          await mouse("mouseReleased", b, { button: "left", buttons: 0, clickCount: 1 })
        }
        await frame()
      },
      async tap(label, at) {
        if (phone) await foldGuide()
        const point = await project(at.x, at.y)
        await reachable(label, point)
        if (phone) {
          await touch("touchStart", [[point.x, point.y]])
          await touch("touchEnd", [])
        } else {
          await mouse("mouseMoved", point)
          await mouse("mousePressed", point, { button: "left", buttons: 1, clickCount: 1 })
          await mouse("mouseReleased", point, { button: "left", buttons: 0, clickCount: 1 })
        }
        await frame()
      },
    }
  }

  const startOpening = async () => {
    assert(await clickButton("Park menu"), "#93: park menu control missing")
    await waitFor("the park menu", `Boolean(document.querySelector('${MENU}'))`, 30)
    await evaluate(`[...document.querySelectorAll('${MENU} button')].find((b) => b.textContent === 'New park')?.click()`)
    await sleep(100)
    assert(await startPickedScenario(context, "first-zoo"), "#93: could not pick the opening scenario")
    await sleep(150)
    await evaluate(`[...document.querySelectorAll('${MENU} button')].find((b) => b.textContent === 'Discard changes')?.click()`)
    await waitFor("the opening park", `Boolean(window.__zooParkView?.ready && document.querySelector('${GUIDE}'))`)
    // The whole meadow fits the frame for this walk; the player may pan freely at any time.
    assert(await clickButton("Reset view"), "#93: Reset view missing")
    await frame()
  }

  const walk = async (phone) => {
    const label = phone ? "phone" : "desktop"
    const input = pointerInput(phone)
    await startOpening()
    assert((await next()) === "build-habitat", `${label}: the guide does not open on the first habitat`, await next())
    await setGuideOpen(true)
    assert((await hint()).includes("Draw habitat"), `${label}: the first hint is missing`, await hint())

    // Skip and restart: the guide hides, the park manager restarts it, nothing else changes.
    const cashBefore = await evaluate(`document.querySelector('.stat strong')?.textContent`)
    assert(await evaluate(`(() => {
      const button = [...document.querySelectorAll('${GUIDE} button')].find((b) => b.textContent === 'Skip guide')
      button?.click()
      return Boolean(button)
    })()`), `${label}: Skip guide missing`)
    await waitFor(`${label} guide to hide after skipping`, `!document.querySelector('${GUIDE}')`, 30)
    assert(await clickButton("Restart guide"), `${label}: Restart guide missing from the park manager`)
    await waitFor(`${label} guide to return`, `Boolean(document.querySelector('${GUIDE}'))`, 30)
    assert(
      (await evaluate(`document.querySelector('.stat strong')?.textContent`)) === cashBefore,
      `${label}: skip/restart changed the park`,
    )
    assert(await clickButton("Ⅱ"), `${label}: pause control missing`)
    if (phone) await setGuideOpen(false)

    // A wrong path: a stub beside the future habitat that does not reach the entrance.
    assert(await clickButton("Path"), `${label}: path tool missing`)
    await input.drag("stub path", { x: 6, y: 6 }, { x: 11, y: 6 })
    await followPointer("Draw habitat", phone)
    await input.drag("habitat fence", { x: 6, y: 1 }, { x: 11, y: 5 })
    assert((await message()).includes("Habitat #1 fenced"), `${label}: the habitat was not fenced`, await message())
    await waitFor(`${label} gate hint`, `document.querySelector('${GUIDE} li.next')?.dataset.objective === 'connect-gate'`, 30)
    await setGuideOpen(true)
    assert((await problem()).includes("no path from the depot"), `${label}: the wrong path has no recovery hint`, await problem())
    await screenshot(`test-results/opening-wrong-path-${label}.png`)
    // Recover: join the stub to the entrance path, then move the gate onto the path side.
    assert(await clickButton("Path"), `${label}: path tool missing`)
    await input.drag("connecting path", { x: 4, y: 7 }, { x: 6, y: 7 })
    await followPointer("Show habitat #1", phone)
    await followPointer("Move habitat #1's gate", phone)
    await input.tap("south fence", { x: 8, y: 5 })
    assert(await clickButton("Place gate here"), `${label}: gate confirmation missing`, await message())
    await waitFor(`${label} zone step`, `document.querySelector('${GUIDE} li.next')?.dataset.objective === 'zone-gate'`, 30)
    await setGuideOpen(true)
    assert((await problem()).startsWith("No work zone"), `${label}: the unzoned gate is not explained`, await problem())

    // Zone, keeper and feed through the guide's pointers.
    await followPointer("Choose Zones", phone)
    await input.drag("work zone", { x: 5, y: 0 }, { x: 12, y: 8 })
    assert((await message()).includes("Work zone #1 created"), `${label}: zone not created`, await message())
    await waitFor(`${label} staffing step`, `document.querySelector('${GUIDE} li.next')?.dataset.objective === 'staff-zone'`, 30)
    await setGuideOpen(true)
    assert((await problem()).includes("hire one at the depot"), `${label}: the unstaffed habitat is not explained`, await problem())
    await followPointer("Open depot", phone)
    await waitFor(`${label} depot inspector`, `document.querySelector('.window-title span')?.textContent === 'Central operations depot'`, 30)
    assert(await clickButton("Hire keeper"), `${label}: Hire keeper missing`)
    await followPointer("Open zone #1", phone)
    await waitFor(`${label} zone inspector`, `document.querySelector('.window-title span')?.textContent.includes('Work zone #1') ?? false`, 30)
    assert(await clickButton("Assign keeper"), `${label}: Assign keeper missing`)
    await waitFor(`${label} feed step`, `document.querySelector('${GUIDE} li.next')?.dataset.objective === 'buy-feed'`, 30)
    await followPointer("Open depot", phone)
    assert(await clickButton("feed crates"), `${label}: feed purchase missing`)

    // An unsuitable group first: one zebra is too lonely, three are a suitable herd.
    await waitFor(`${label} adoption step`, `document.querySelector('${GUIDE} li.next')?.dataset.objective === 'adopt-group'`, 30)
    await followPointer("Show habitat #1", phone)
    await waitFor(`${label} habitat inspector`, `document.querySelector('.window-title span')?.textContent === 'Habitat #1'`, 30)
    assert(await clickButton("Zebra"), `${label}: zebra adoption missing`)
    await setGuideOpen(true)
    await waitFor(`${label} group hint`, `document.querySelector('${GUIDE} .tutorial-problem')?.textContent.includes('need at least 3 together') ?? false`, 30)
    if (phone) await setGuideOpen(false)
    for (let count = 0; count < 2; count += 1) assert(await clickButton("Zebra"), `${label}: zebra adoption missing`)
    await waitFor(`${label} shelter step`, `document.querySelector('${GUIDE} li.next')?.dataset.objective === 'add-shelter'`, 30)
    assert(await clickButton("Add basic shelter"), `${label}: shelter missing`)

    // Care work from the species routine, one small priority edit, then time runs.
    assert(await clickButton("4×"), `${label}: speed control missing`)
    await waitFor(`${label} care work`, `document.querySelector('${GUIDE} [data-objective="see-care-work"]')?.classList.contains('done') ?? false`, 400)
    assert(await evaluate(`(() => {
      const button = [...document.querySelectorAll('button[aria-label$=" priority"]')].find((candidate) => !candidate.disabled)
      button?.click()
      return Boolean(button)
    })()`), `${label}: no care priority control was enabled`)
    await waitFor(`${label} priority edit`, `document.querySelector('${GUIDE} [data-objective="edit-schedule"]')?.classList.contains('done') ?? false`, 30)
    await waitFor(`${label} guests`, `document.querySelector('${GUIDE} [data-objective="first-guests"]')?.classList.contains('done') ?? false`, 600)
    await closeInspector()
    await followPointer("Choose Food stand", phone)
    await input.tap("food stand", { x: 5, y: 8 })
    assert((await message()).includes("stand #1 built"), `${label}: the stand was not built`, await message())
    await screenshot(`test-results/opening-operating-${label}.png`)
    assert(await clickButton("Inspect"), `${label}: inspect tool missing`)

    // A keeper finishes care and welfare settles: the guide completes and hides itself.
    await waitFor(`${label} guide completion`, `!document.querySelector('${GUIDE}')`, 1500)
    await closeInspector()
    await waitFor(`${label} restart control`, `[...document.querySelectorAll('button')].some((b) => b.textContent === 'Restart guide')`, 30)

    // Save, reload the page and resume: the finished guide stays finished.
    assert(await clickButton("Park menu"), `${label}: park menu control missing`)
    await waitFor("the park menu", `Boolean(document.querySelector('${MENU}'))`, 30)
    await evaluate(`[...document.querySelectorAll('${MENU} button')].find((b) => b.textContent.startsWith('Save'))?.click()`)
    await waitFor(`${label} save`, `document.querySelector('.library-message')?.textContent.startsWith('Saved “') ?? false`, 50)
    await cdp.send("Page.navigate", { url: previewUrl })
    await waitFor(`${label} start screen`, `[...document.querySelectorAll('${START} button')].some((b) => b.textContent.startsWith('Continue'))`, 200)
    await evaluate(`[...document.querySelectorAll('${START} button')].find((b) => b.textContent.startsWith('Continue'))?.click()`)
    await waitFor(`${label} resumed park`, `Boolean(window.__zooParkView?.ready) && [...document.querySelectorAll('button')].some((b) => b.textContent === 'Restart guide')`, 200)
    assert(!(await evaluate(`Boolean(document.querySelector('${GUIDE}'))`)), `${label}: the finished guide came back after reload`)
    await screenshot(`test-results/opening-resumed-${label}.png`)
  }

  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })
  await walk(false)
  await cdp.send("Emulation.setDeviceMetricsOverride", { width: 390, height: 844, deviceScaleFactor: 2, mobile: true })
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: true, maxTouchPoints: 5 })
  await walk(true)
  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })
  console.log("#93 opening scenario acceptance passed: guided walk to an operating zoo, recovery hints, skip/restart and save/reload on desktop and phone")
}
