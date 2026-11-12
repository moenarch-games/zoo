// #104 browser acceptance: physical keeper care falls behind when a work zone is understaffed,
// the player corrects the staffing, and care measurably recovers; desktop and phone.
//
// Called by capture-park-view-proof.mjs against the same production preview and real headless
// Chromium. The player opens the authored park (crates/zoo-core/tests/fixtures/
// keeper-care-park.json, written by zoo-core's own test) through the real file import: three
// hungry capybara habitats in work zone #1 with one keeper, while keepers #2 and #3 stand idle
// in an empty corner zone. The workspace's care backlog (rendered from zoo-core's snapshot)
// counts missed care over a stretch of park time while the lone keeper walks crates from the
// depot to each habitat. The player opens zone #1 from the staff table and assigns both idle
// keepers with the zone inspector's own control; over an equal stretch, less care is missed and
// more is done. Park time and every count come from zoo-core.

import { readFileSync, writeFileSync } from "node:fs"

const MENU = `[aria-label="Park menu"][role="dialog"]`
const WORKSPACE = `section[aria-label="Management workspace"]`
const BACKLOG = `${WORKSPACE} dl.care-backlog`
const FIXTURE = new URL("../../../crates/zoo-core/tests/fixtures/keeper-care-park.json", import.meta.url)
/** Park minutes each comparison window lasts. */
const WINDOW = 240

export async function runKeeperCareAcceptance(context) {
  const { cdp, evaluate, assert, sleep, screenshot, clickButton, frame } = context
  const waitFor = async (description, expression, attempts = 150) => {
    for (let attempt = 0; attempt < attempts; attempt += 1) {
      if (await evaluate(expression)) return
      await sleep(100)
    }
    const state = await evaluate(`({
      message: document.querySelector('.message')?.textContent ?? null,
      panel: document.querySelector('.side-panel .window-title span')?.textContent ?? null,
      backlog: document.querySelector('${BACKLOG}')?.textContent ?? null,
    })`)
    throw new Error(`#104 acceptance timed out waiting for ${description}: ${JSON.stringify(state)}`)
  }
  const tick = () => evaluate(`JSON.parse(window.__zooDiagnostics.exportText()).session.clock.tick`)
  const backlog = (name) =>
    evaluate(`Number(document.querySelector('${BACKLOG}')?.dataset.${name} ?? NaN)`)

  const openStaffTab = async () => {
    if (!(await evaluate(`Boolean(document.querySelector('${WORKSPACE}'))`))) {
      assert(await clickButton("Management workspace"), "#104: Manage control missing")
      await waitFor("the workspace", `Boolean(document.querySelector('${WORKSPACE}'))`, 30)
    }
    await evaluate(
      `[...document.querySelectorAll('${WORKSPACE} [role="tab"]')].find((b) => b.textContent.startsWith('Staff'))?.click()`,
    )
    await frame()
    await waitFor("the care backlog", `Boolean(document.querySelector('${BACKLOG}'))`, 30)
  }

  /** Runs the park at 4× for `minutes` park minutes; returns the care done and missed then. */
  const careOver = async (minutes) => {
    await openStaffTab()
    const start = await tick()
    const completed = await backlog("careCompleted")
    const missed = await backlog("careMissed")
    assert(await clickButton("4×"), "#104: speed control missing")
    await waitFor(
      `${minutes} park minutes`,
      `JSON.parse(window.__zooDiagnostics.exportText()).session.clock.tick >= ${start + minutes}`,
      1800,
    )
    assert(await clickButton("Ⅱ"), "#104: pause control missing")
    await frame()
    return {
      completed: (await backlog("careCompleted")) - completed,
      missed: (await backlog("careMissed")) - missed,
      elapsed: (await tick()) - start,
    }
  }

  // --- Open the understaffed park through the real import ---------------------------------
  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })
  const name = "keeper-care"
  const path = `${process.cwd()}/test-results/${name}.zoo-save.json`
  writeFileSync(path, readFileSync(FIXTURE, "utf8"))
  assert(await clickButton("Park menu"), "#104: park menu control missing")
  await waitFor("the park menu", `Boolean(document.querySelector('${MENU}'))`, 30)
  await evaluate(`[...document.querySelectorAll('${MENU} button')].find((b) => b.textContent.startsWith('Load park'))?.click()`)
  await sleep(150)
  const { root } = await cdp.send("DOM.getDocument", { depth: -1 })
  const { nodeId } = await cdp.send("DOM.querySelector", { nodeId: root.nodeId, selector: 'input[type="file"]' })
  assert(nodeId, "#104: the park file input is missing")
  await cdp.send("DOM.setFileInputFiles", { nodeId, files: [path] })
  await waitFor(
    "the import",
    `[...document.querySelectorAll('.library-slots > li')].some((item) => item.dataset.slot === ${JSON.stringify(name)})`,
    60,
  )
  await evaluate(`(() => {
    const item = [...document.querySelectorAll('.library-slots > li')].find((entry) => entry.dataset.slot === ${JSON.stringify(name)})
    ;[...(item?.querySelectorAll('button') ?? [])].find((b) => b.textContent === 'Load')?.click()
  })()`)
  await sleep(200)
  await evaluate(`[...document.querySelectorAll('${MENU} button')].find((b) => b.textContent === 'Discard changes')?.click()`)
  await waitFor("the park", `(document.querySelector('.message')?.textContent ?? '').startsWith('Park loaded')`, 100)
  await waitFor("the renderer", `Boolean(window.__zooParkView?.ready && window.__zooParkView.camera())`, 200)
  assert(await clickButton("Ⅱ"), "#104: pause control missing")

  // --- Overload: one keeper walks crates from the depot to three hungry habitats -----------
  const overloaded = await careOver(WINDOW)
  assert(overloaded.missed > 0, "#104: the lone keeper kept up with three habitats", overloaded)
  const tasks = await evaluate(
    `[...document.querySelectorAll('${WORKSPACE} table[aria-label="Tasks"] tbody tr')].map((row) => row.textContent)`,
  )
  assert(
    tasks.some((row) => /Queued|Blocked|Collecting stock|Traveling|Crew reserved|Servicing/.test(row)),
    "#104: the task table shows no care phase",
    tasks,
  )
  const keeperStatus = await evaluate(
    `[...document.querySelectorAll('${WORKSPACE} table[aria-label="Staff"] tbody tr')].map((row) => row.textContent)`,
  )
  assert(keeperStatus.some((row) => row.startsWith("Keeper #2") && row.includes("Zone #2")), "#104: keeper #2 is not idle in zone #2", keeperStatus)
  await screenshot("test-results/keepers-overloaded-desktop.png")

  // --- Correct the staffing: assign both idle keepers to zone #1 from its inspector --------
  assert(
    await evaluate(`(() => {
      const row = [...document.querySelectorAll('${WORKSPACE} table[aria-label="Staff"] tbody tr')].find((r) => r.textContent.startsWith('Keeper #1'))
      row?.click()
      return Boolean(row)
    })()`),
    "#104: the staff table does not list keeper #1",
  )
  await frame()
  if (await evaluate(`Boolean(document.querySelector('${WORKSPACE}'))`)) {
    await evaluate(`document.querySelector('${WORKSPACE} .window-title button')?.click()`)
    await frame()
  }
  await waitFor(
    "zone #1's inspector",
    `(document.querySelector('.side-panel .window-title span')?.textContent ?? '').includes('Work zone #1')`,
    30,
  )
  for (const keeper of [2, 3]) {
    assert(await clickButton("Assign keeper"), `#104: Assign keeper missing for keeper #${keeper}`)
    await waitFor(`keeper #${keeper}'s assignment`, `(document.querySelector('.message')?.textContent ?? '').includes('Keeper #${keeper} assigned to work zone #1')`, 30)
  }
  const recovered = await careOver(WINDOW)
  assert(
    recovered.missed * overloaded.elapsed < overloaded.missed * recovered.elapsed,
    "#104: three keepers missed as much care as one",
    { overloaded, recovered },
  )
  assert(
    recovered.completed * overloaded.elapsed > overloaded.completed * recovered.elapsed,
    "#104: three keepers did no more care than one",
    { overloaded, recovered },
  )
  const status = await evaluate(
    `[...document.querySelectorAll('${WORKSPACE} table[aria-label="Staff"] tbody tr')].filter((row) => row.textContent.startsWith('Keeper')).map((row) => row.textContent)`,
  )
  assert(status.every((row) => row.includes("Zone #1")), "#104: not every keeper works zone #1", status)
  await screenshot("test-results/keepers-recovered-desktop.png")

  // --- Phone: the backlog and task phases fit a touch viewport -----------------------------
  await cdp.send("Emulation.setDeviceMetricsOverride", { width: 390, height: 844, deviceScaleFactor: 2, mobile: true })
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: true, maxTouchPoints: 5 })
  await sleep(400)
  await openStaffTab()
  const fits = await evaluate(`(() => {
    const box = document.querySelector('${BACKLOG}').getBoundingClientRect()
    return box.width > 0 && box.right <= window.innerWidth + 1
  })()`)
  assert(fits, "#104: the care backlog overflows the phone viewport")
  await screenshot("test-results/keepers-phone.png")
  await cdp.send("Emulation.clearDeviceMetricsOverride")
  await cdp.send("Emulation.setTouchEmulationEnabled", { enabled: false })
  console.log(
    `#104 keeper-care acceptance passed: one keeper missed ${overloaded.missed} and did ${overloaded.completed} care jobs in ${overloaded.elapsed} park minutes; with keepers #2 and #3 moved into the zone, ${recovered.missed} were missed and ${recovered.completed} done in ${recovered.elapsed}; ${tasks.length} open tasks were listed with their phases`,
  )
}
