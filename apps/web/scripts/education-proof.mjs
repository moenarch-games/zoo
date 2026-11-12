// #116 browser acceptance: an education objective is completed through park operations, and its
// effect on guests and finances is shown.
//
// Called by capture-park-view-proof.mjs against the same production preview and real headless
// Chromium. The player opens the Riverside zoo half an hour into its first day
// (crates/zoo-core/tests/fixtures/education-park.json, written by zoo-core's own test: the
// living-starter scenario with a second keeper; nobody has learned anything yet) through the
// real file import. The scenario asks the player to teach 12 guests about the animals and
// promises a community learning grant once two keeper talks have been given. The player:
//   1. opens Manage › Education, which says what to build and do next;
//   2. builds an education sign beside the entrance path by real canvas picking; zoo-core says
//      it teaches about the capybaras of habitat #1 and shows its sourced fact;
//   3. schedules daily keeper talks at both habitats in the Education panel;
//   4. runs the park until guests have read the sign and listened to the talks: the objective
//      ticks, the grant is paid once, leaving guests donate, and the Finances ledger shows the
//      grant and the donations as income.
// On a phone the panel is used by touch: a talk is canceled with a tap. Every figure, status and
// reason comes from zoo-core's snapshot.

import { readFileSync, writeFileSync } from "node:fs"

const MENU = `[aria-label="Park menu"][role="dialog"]`
const WORKSPACE = `section[aria-label="Management workspace"]`
const PANEL = `${WORKSPACE} .education-panel`
const FIXTURE = new URL("../../../crates/zoo-core/tests/fixtures/education-park.json", import.meta.url)
const SIGN = { x: 3, y: 11 }

export async function runEducationAcceptance(context) {
  const { cdp, evaluate, assert, sleep, screenshot, clickButton, frame, tile, mouse, key } = context
  const waitFor = async (description, expression, attempts = 150) => {
    for (let attempt = 0; attempt < attempts; attempt += 1) {
      if (await evaluate(expression)) return
      await sleep(100)
    }
    const state = await evaluate(`({
      message: document.querySelector('.message')?.textContent ?? null,
      panel: document.querySelector('${PANEL}')?.textContent ?? null,
    })`)
    throw new Error(`#116 acceptance timed out waiting for ${description}: ${JSON.stringify(state)}`)
  }
  const message = () => evaluate(`document.querySelector('.message')?.textContent ?? ''`)
  const openTab = async (name) => {
    if (!(await evaluate(`Boolean(document.querySelector('${WORKSPACE}'))`))) {
      assert(await clickButton("Management workspace"), "#116: Manage control missing")
      await waitFor("the workspace", `Boolean(document.querySelector('${WORKSPACE}'))`, 30)
    }
    await evaluate(`[...document.querySelectorAll('${WORKSPACE} [role="tab"]')].find((b) => b.textContent.startsWith(${JSON.stringify(name)}))?.click()`)
    await frame()
  }
  const closeWorkspace = async () => {
    await evaluate(`document.querySelector('${WORKSPACE} .window-title button')?.click()`)
    await frame()
  }
  const report = () =>
    evaluate(`(() => {
      const panel = document.querySelector('${PANEL}')
      if (!panel) return null
      const data = panel.dataset
      return {
        educated: Number(data.educated),
        talksGiven: Number(data.talksGiven),
        donations: Number(data.donations),
        grants: Number(data.grants),
        learners: Number(data.learners),
        grantAwarded: panel.querySelector('[data-grant="community-learning-grant"]')?.dataset.awarded ?? null,
        reasons: [...panel.querySelectorAll('.education-reasons li')].map((item) => item.textContent),
        signs: [...panel.querySelectorAll('.sign-list li')].map((item) => ({
          status: item.dataset.status,
          text: item.textContent,
          source: item.querySelector('.sign-fact a')?.getAttribute('href') ?? null,
        })),
        talks: Object.fromEntries([...panel.querySelectorAll('.talk-row')].map((row) => [row.dataset.habitat, {
          status: row.dataset.status,
          last: row.querySelector('.talk-last')?.dataset.given ?? null,
          text: row.textContent,
        }])),
      }
    })()`)
  const objectiveDone = (id) =>
    evaluate(`document.querySelector('li[data-objective="${id}"]')?.classList.contains('done') ?? null`)
  /** Types into a talk form field of one habitat's row. */
  const typeTalk = async (habitat, field, text) => {
    const box = await evaluate(`(() => {
      const input = document.querySelector('${PANEL} .talk-row[data-habitat="${habitat}"] input[data-talk-input="${field}"]')
      if (!input) return null
      input.scrollIntoView({ block: "center" })
      const r = input.getBoundingClientRect()
      return { x: r.left + r.width / 2, y: r.top + r.height / 2 }
    })()`)
    assert(box, `#116: habitat #${habitat}'s ${field} field is missing`)
    await frame()
    await cdp.send("Input.dispatchMouseEvent", { type: "mousePressed", x: box.x, y: box.y, button: "left", buttons: 1, clickCount: 1 })
    await cdp.send("Input.dispatchMouseEvent", { type: "mouseReleased", x: box.x, y: box.y, button: "left", buttons: 0, clickCount: 1 })
    await evaluate(`document.querySelector('${PANEL} .talk-row[data-habitat="${habitat}"] input[data-talk-input="${field}"]').select()`)
    await cdp.send("Input.insertText", { text })
    await frame()
  }
  const rowButton = (habitat, text) =>
    evaluate(`(() => {
      const row = document.querySelector('${PANEL} .talk-row[data-habitat="${habitat}"]')
      const button = [...(row?.querySelectorAll('button') ?? [])].find((b) => b.textContent === ${JSON.stringify(text)})
      if (!button || button.disabled) return false
      button.click()
      return true
    })()`)
  const settleCamera = async () => {
    let last = ""
    for (let attempt = 0; attempt < 60; attempt += 1) {
      const now = JSON.stringify(await evaluate("window.__zooParkView.camera()"))
      if (now === last) return
      last = now
      await sleep(150)
    }
  }
  const ledgerLine = (name) =>
    evaluate(`(() => {
      const row = [...document.querySelectorAll('${WORKSPACE} tr')].find((r) => r.cells[0]?.textContent === ${JSON.stringify(name)})
      return row ? row.cells[2]?.textContent ?? null : null
    })()`)

  // --- Open the Riverside zoo through the real file import -------------------------------------
  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })
  const name = "education-park"
  const path = `${process.cwd()}/test-results/${name}.zoo-save.json`
  writeFileSync(path, readFileSync(FIXTURE, "utf8"))
  assert(await clickButton("Park menu"), "#116: park menu control missing")
  await waitFor("the park menu", `Boolean(document.querySelector('${MENU}'))`, 30)
  await evaluate(`[...document.querySelectorAll('${MENU} button')].find((b) => b.textContent.startsWith('Load park'))?.click()`)
  await sleep(150)
  const { root } = await cdp.send("DOM.getDocument", { depth: -1 })
  const { nodeId } = await cdp.send("DOM.querySelector", { nodeId: root.nodeId, selector: 'input[type="file"]' })
  assert(nodeId, "#116: the park file input is missing")
  await cdp.send("DOM.setFileInputFiles", { nodeId, files: [path] })
  await waitFor("the import", `[...document.querySelectorAll('.library-slots > li')].some((item) => item.dataset.slot === ${JSON.stringify(name)})`, 60)
  await evaluate(`(() => {
    const item = [...document.querySelectorAll('.library-slots > li')].find((entry) => entry.dataset.slot === ${JSON.stringify(name)})
    ;[...(item?.querySelectorAll('button') ?? [])].find((b) => b.textContent === 'Load')?.click()
  })()`)
  await sleep(200)
  await evaluate(`[...document.querySelectorAll('${MENU} button')].find((b) => b.textContent === 'Discard changes')?.click()`)
  await waitFor("the park", `(document.querySelector('.message')?.textContent ?? '').startsWith('Park loaded')`, 100)
  await waitFor("the renderer", `Boolean(window.__zooParkView?.ready && window.__zooParkView.camera())`, 200)
  assert(await clickButton("Ⅱ"), "#116: pause control missing")

  // --- 1. The report says what to do ------------------------------------------------------------
  await openTab("Education")
  await waitFor("the education panel", `Boolean(document.querySelector('${PANEL}'))`, 30)
  const start = await report()
  assert(start.educated === 0 && start.signs.length === 0, "#116: the park already teaches", start)
  assert(start.reasons.some((reason) => /education sign/.test(reason)), "#116: the report does not suggest a sign", start)
  assert(start.grantAwarded === "false", "#116: the grant is not offered or already paid", start)
  assert((await objectiveDone("teach-guests")) === false, "#116: the education objective is missing or done")
  await screenshot("test-results/education-start-desktop.png")
  await closeWorkspace()

  // --- 2. Build a sign beside the entrance path ------------------------------------------------
  assert(await clickButton("Education sign ·"), "#116: the sign tool is missing")
  await key("Home")
  await settleCamera()
  const point = await tile(SIGN.x, SIGN.y)
  assert(point.visible, "#116: the sign plot is off screen", point)
  await mouse("mouseMoved", point)
  await mouse("mousePressed", point, { button: "left", buttons: 1, clickCount: 1 })
  await mouse("mouseReleased", point, { button: "left", buttons: 0, clickCount: 1 })
  await frame()
  assert(/Education sign #\d+ built/.test(await message()), "#116: the sign was not built", await message())
  await key("Escape")
  await openTab("Education")
  await waitFor("the sign", `Boolean(document.querySelector('${PANEL} .sign-list li'))`, 30)
  const signed = await report()
  assert(signed.signs[0].status === "teaching", "#116: the sign does not teach", signed.signs)
  assert(/capybara/i.test(signed.signs[0].text), "#116: the sign is not about the capybaras", signed.signs)
  assert(signed.signs[0].source?.startsWith("https://"), "#116: the sign's fact has no source", signed.signs)

  // --- 3. Schedule a daily talk at each habitat -------------------------------------------------
  for (const [habitat, at] of [[1, "10:00"], [2, "10:15"]]) {
    await typeTalk(habitat, "start", at)
    await typeTalk(habitat, "duration", "20")
    await typeTalk(habitat, "places", "6")
    assert(await rowButton(habitat, "Schedule talk"), `#116: habitat #${habitat} cannot schedule a talk`)
    await waitFor(`habitat #${habitat}'s talk`, `document.querySelector('${PANEL} .talk-row[data-habitat="${habitat}"]')?.dataset.status !== 'none'`, 30)
  }
  await screenshot("test-results/education-scheduled-desktop.png")

  // --- 4. Run the park: guests learn, talks are given, the grant and donations arrive -----------
  assert(await clickButton("4×"), "#116: speed control missing")
  await waitFor(
    "taught guests, two talks, the grant and a donation",
    `(() => {
      const panel = document.querySelector('${PANEL}')
      return Boolean(panel) && Number(panel.dataset.educated) >= 12 && Number(panel.dataset.talksGiven) >= 2
        && Number(panel.dataset.donations) > 0
        && panel.querySelector('[data-grant="community-learning-grant"]')?.dataset.awarded === 'true'
    })()`,
    3000,
  )
  assert(await clickButton("Ⅱ"), "#116: pause control missing")
  await frame()
  const after = await report()
  assert(after.grants === 150_000, "#116: the grant was not paid exactly once", after)
  assert(after.talks["1"].last === "true" || after.talks["2"].last === "true", "#116: no talk was given to the end", after.talks)
  assert((await objectiveDone("teach-guests")) === true, "#116: the education objective did not tick")
  await screenshot("test-results/education-taught-desktop.png")

  // The effect on finances: the grant and the donations are income lines in the ledger.
  await openTab("Finances")
  await waitFor("the ledger", `Boolean([...document.querySelectorAll('${WORKSPACE} tr')].find((r) => r.cells[0]?.textContent === 'Grants'))`, 30)
  const grants = await ledgerLine("Grants")
  const donations = await ledgerLine("Donations")
  assert(grants === "$1,500", "#116: the ledger does not show the grant", { grants })
  assert(donations !== null && donations !== "$0", "#116: the ledger does not show donations", { donations })
  await screenshot("test-results/education-finances-desktop.png")

  // --- Phone: a talk canceled by touch ---------------------------------------------------------
  await cdp.send("Emulation.setDeviceMetricsOverride", { width: 390, height: 844, deviceScaleFactor: 2, mobile: true })
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: true, maxTouchPoints: 5 })
  await sleep(400)
  await openTab("Education")
  await waitFor("the education panel on the phone", `Boolean(document.querySelector('${PANEL}'))`, 30)
  const box = await evaluate(`(() => {
    const row = document.querySelector('${PANEL} .talk-row[data-habitat="2"]')
    const button = [...(row?.querySelectorAll('button') ?? [])].find((b) => b.textContent === 'Cancel talk')
    if (!button) return null
    button.scrollIntoView({ block: "center" })
    const r = button.getBoundingClientRect()
    return { x: r.left + r.width / 2, y: r.top + r.height / 2 }
  })()`)
  assert(box, "#116: no Cancel talk control on the phone")
  await frame()
  const hit = await evaluate(`document.elementFromPoint(${box.x}, ${box.y})?.textContent ?? null`)
  assert(hit === "Cancel talk", "#116: Cancel talk is covered on the phone", hit)
  await cdp.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: box.x, y: box.y, id: 0 }] })
  await cdp.send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] })
  await waitFor("the canceled talk", `document.querySelector('${PANEL} .talk-row[data-habitat="2"]')?.dataset.status === 'none'`, 30)
  await screenshot("test-results/education-phone.png")
  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })
  await closeWorkspace()
  console.log(
    `#116 education acceptance passed: a sign by the entrance path taught guests about the capybaras with a sourced fact, daily keeper talks were scheduled and given, ${after.educated} guests were taught (objective ticked, ${after.learners} learning now), the $1,500 community learning grant was paid once and guests donated $${(after.donations / 100).toFixed(2)}, both shown as ledger income; a talk was canceled by touch on a phone`,
  )
}
