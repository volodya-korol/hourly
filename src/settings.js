const { invoke } = window.__TAURI__.core
const { open, save } = window.__TAURI__.dialog
const { LogicalSize } = window.__TAURI__.dpi
const appWindow = window.__TAURI__.window.getCurrentWindow()

const FORMAT_DOCS = 'https://docs.rs/chrono/latest/chrono/format/strftime/index.html'
const SAVE_DELAY_MS = 400
const WINDOW_WIDTH = 500
const CUSTOM = 'custom'
const DEFAULT_MIN_LENGTH = 50
const MAX_MIN_LENGTH = 500

const $ = (id) => document.getElementById(id)

const rootInput = $('root')
const clearButton = $('clear')
const notesOptions = $('notesOptions')
const templateInput = $('template')
const markerInput = $('marker')
const minLengthInput = $('minLength')
const status = $('status')
const preview = $('preview')
const previewRows = $('previewRows')
const previewError = $('previewError')
const todayPath = $('todayPath')
const tomorrowPath = $('tomorrowPath')
const nextMonthPath = $('nextMonthPath')
const presetSelect = $('preset')
const detectButton = $('detect')
const detectError = $('detectError')
const customBox = $('customBox')

let saveTimer = null
let lastSaved = ''
let presets = []

// ---------- Window size ----------
// The window is exactly as tall as the open tab: no scrollbar and no empty space.
// On a small screen it stops at the screen height and the tab scrolls instead.
// It starts hidden and is shown after the first fit, so it never visibly jumps.
const natural = $('natural')
const titlebar = document.querySelector('.titlebar')
const tabbar = document.querySelector('.tabs')
const contentBox = document.querySelector('.content')
let shown = false

async function fitWindow() {
  const padding = getComputedStyle(contentBox)
  const wanted = Math.ceil(
    2 +
      titlebar.offsetHeight +
      tabbar.offsetHeight +
      parseFloat(padding.paddingTop) +
      parseFloat(padding.paddingBottom) +
      natural.offsetHeight,
  )
  const height = Math.min(wanted, Math.max(360, window.screen.availHeight - 24))
  await appWindow.setSize(new LogicalSize(WINDOW_WIDTH, height))
  if (!shown) {
    shown = true
    await appWindow.center()
    await appWindow.show()
    await appWindow.setFocus()
  }
}

new ResizeObserver(() => fitWindow()).observe(natural)

// ---------- Tabs ----------
const tabs = [...document.querySelectorAll('.tab')]
const panels = [...document.querySelectorAll('.panel')]

function showTab(name) {
  for (const tab of tabs) {
    const active = tab.dataset.tab === name
    tab.classList.toggle('active', active)
    tab.setAttribute('aria-selected', String(active))
  }
  for (const panel of panels) panel.hidden = panel.id !== `panel-${name}`
  // The status line belongs to what was just done on the previous tab.
  setStatus('')
  if (name === 'general') refreshStats()
}

for (const tab of tabs) tab.addEventListener('click', () => showTab(tab.dataset.tab))

// ---------- Saving ----------
function setStatus(text, kind = '') {
  status.textContent = text
  status.className = `status ${kind}`.trim()
}

// A blank field means "the default"; anything that is not a whole number from 0
// to 500 is null, which stops the save with a message.
function minLengthValue() {
  const text = minLengthInput.value.trim()
  if (text === '') return DEFAULT_MIN_LENGTH
  const number = Number(text)
  return Number.isInteger(number) && number >= 0 && number <= MAX_MIN_LENGTH ? number : null
}

// The notes folder is optional: null means entries are kept only on this computer.
function form() {
  return {
    journalRoot: rootInput.value.trim() || null,
    pathTemplate: templateInput.value,
    marker: markerInput.value,
    minLength: minLengthValue(),
  }
}

// Shows where today's, tomorrow's and next month's notes will go, or why the
// template is wrong. Returns false when the template cannot be used.
async function updatePreview() {
  const { journalRoot, pathTemplate } = form()
  if (!journalRoot) {
    preview.hidden = true
    return true
  }
  try {
    const result = await invoke('preview_path', { journalRoot, pathTemplate })
    todayPath.textContent = result.today
    tomorrowPath.textContent = result.tomorrow
    nextMonthPath.textContent = result.nextMonth
    previewRows.hidden = false
    previewError.hidden = true
    preview.hidden = false
    return true
  } catch (error) {
    previewError.textContent = String(error)
    previewRows.hidden = true
    previewError.hidden = false
    preview.hidden = false
    return false
  }
}

// Settings save themselves: there is no Save button. A field that was left
// empty is filled with its default once it loses focus.
async function commit({ refill = false } = {}) {
  clearTimeout(saveTimer)
  saveTimer = null

  const settings = form()
  if (settings.minLength === null) {
    setStatus(`The minimum length is a whole number from 0 to ${MAX_MIN_LENGTH}`, 'error')
    return
  }
  if (settings.journalRoot && !(await updatePreview())) {
    setStatus('Fix the date format to save', 'error')
    return
  }

  const fingerprint = JSON.stringify(settings)
  if (fingerprint !== lastSaved) {
    try {
      const saved = await invoke('save_settings', { settings })
      lastSaved = JSON.stringify({
        journalRoot: saved.journalRoot,
        pathTemplate: saved.pathTemplate,
        marker: saved.marker,
        minLength: saved.minLength,
      })
      if (refill) {
        if (!templateInput.value.trim()) templateInput.value = saved.pathTemplate
        if (!markerInput.value.trim()) markerInput.value = saved.marker
        if (!minLengthInput.value.trim()) minLengthInput.value = saved.minLength
        await updatePreview()
      }
    } catch (error) {
      setStatus(String(error), 'error')
      return
    }
  }
  setStatus('All changes saved', 'success')
}

function scheduleCommit() {
  setStatus('Saving…')
  clearTimeout(saveTimer)
  saveTimer = setTimeout(commit, SAVE_DELAY_MS)
}

async function closeWindow() {
  if (saveTimer) await commit()
  appWindow.close()
}

// ---------- Notes folder ----------
// The layout list shows each layout with today's path as its example, so the
// choice is made by looking at real paths instead of reading token syntax.
function fillPresets() {
  presetSelect.replaceChildren(
    ...presets.map((preset) => {
      const option = document.createElement('option')
      option.value = preset.id
      // The path comes first: it is what people recognise their own notes by.
      option.textContent = `${preset.example}  ·  ${preset.label}`
      return option
    }),
    Object.assign(document.createElement('option'), {
      value: CUSTOM,
      textContent: 'Custom format…',
    }),
  )
}

// Selects the layout that matches `template`, or "Custom format" (with its text
// field shown) when it is not one of the ready-made ones.
function syncSelect() {
  const match = presets.find((preset) => preset.template === templateInput.value)
  presetSelect.value = match ? match.id : CUSTOM
  customBox.hidden = presetSelect.value !== CUSTOM
}

// Layout, preview and marker only matter once a folder is chosen.
function updateNotesVisibility() {
  const hasFolder = rootInput.value.trim() !== ''
  notesOptions.hidden = !hasFolder
  clearButton.hidden = !hasFolder
}

function showDetectError(message) {
  detectError.textContent = message || ''
  detectError.hidden = !message
}

// Used by the layout list and by "Detect from a note".
async function useTemplate(template) {
  templateInput.value = template
  syncSelect()
  await commit()
}

$('pick').addEventListener('click', async () => {
  const picked = await open({
    directory: true,
    multiple: false,
    title: 'Choose the folder for your notes',
    defaultPath: rootInput.value || undefined,
  })
  if (typeof picked === 'string') {
    rootInput.value = picked
    showDetectError('')
    updateNotesVisibility()
    await commit()
  }
})

clearButton.addEventListener('click', async () => {
  rootInput.value = ''
  showDetectError('')
  updateNotesVisibility()
  await commit()
  setStatus('Entries are now kept only on this computer', 'success')
})

presetSelect.addEventListener('change', async () => {
  showDetectError('')
  const preset = presets.find((item) => item.id === presetSelect.value)
  if (preset) {
    await useTemplate(preset.template)
  } else {
    customBox.hidden = false
    templateInput.focus()
  }
})

detectButton.addEventListener('click', async () => {
  showDetectError('')
  const journalRoot = rootInput.value.trim()
  const picked = await open({
    directory: false,
    multiple: false,
    title: 'Choose one of your daily notes',
    defaultPath: journalRoot,
    filters: [{ name: 'Markdown notes', extensions: ['md'] }],
  })
  if (typeof picked !== 'string') return
  try {
    const template = await invoke('infer_template', { journalRoot, notePath: picked })
    await useTemplate(template)
    const name = picked.split(/[\\/]/).pop()
    setStatus(`Pattern found in ${name}. All changes saved`, 'success')
  } catch (error) {
    showDetectError(String(error))
  }
})

templateInput.addEventListener('input', () => {
  updatePreview()
  scheduleCommit()
})
markerInput.addEventListener('input', scheduleCommit)
minLengthInput.addEventListener('input', scheduleCommit)
templateInput.addEventListener('blur', () => commit({ refill: true }))
markerInput.addEventListener('blur', () => commit({ refill: true }))
minLengthInput.addEventListener('blur', () => commit({ refill: true }))

// ---------- General ----------
async function refreshStats() {
  try {
    const { count, since } = await invoke('entry_stats')
    $('statsText').textContent =
      count === 0
        ? 'No entries yet'
        : `${count} ${count === 1 ? 'entry' : 'entries'} since ${since}`
  } catch (error) {
    $('statsText').textContent = String(error)
  }
}

$('showEntries').addEventListener('click', () =>
  invoke('reveal_entries').catch((error) => setStatus(String(error), 'error')),
)

// Start with Windows is not a saved setting: Windows holds the answer, so the
// box is read from it and a click is applied to it straight away.
const autostartBox = $('autostart')

invoke('get_autostart').then(
  (enabled) => {
    autostartBox.checked = enabled
  },
  (error) => setStatus(String(error), 'error'),
)

autostartBox.addEventListener('change', async () => {
  const wanted = autostartBox.checked
  try {
    await invoke('set_autostart', { enabled: wanted })
    setStatus(wanted ? 'Hourly will start with Windows' : 'Hourly will not start with Windows', 'success')
  } catch (error) {
    autostartBox.checked = !wanted
    setStatus(String(error), 'error')
  }
})

// ---------- Export ----------
const exportFormat = $('exportFormat')
const exportRange = $('exportRange')
const exportPrompt = $('exportPrompt')
const exportCopy = $('exportCopy')
const exportSave = $('exportSave')
const exportMsg = $('exportMsg')

const FORMAT_HINTS = {
  markdown: 'A heading for each day. Easy to read, and the best choice for pasting into a chat.',
  text: 'One line per entry. The smallest, good for long periods.',
  json: 'Structured data for scripts and tools. The instruction becomes a field of its own.',
}
const EXTENSIONS = { markdown: 'md', text: 'txt', json: 'json' }
const FORMAT_NAMES = { markdown: 'Markdown', text: 'Text', json: 'JSON' }

function showFormatHint() {
  $('formatHint').textContent = FORMAT_HINTS[exportFormat.value]
}
exportFormat.addEventListener('change', showFormatHint)
showFormatHint()

function exportMessage(text, kind = '') {
  exportMsg.textContent = text
  exportMsg.className = `field-msg ${kind}`.trim()
}

const exportArgs = () => ({
  format: exportFormat.value,
  range: exportRange.value,
  withPrompt: exportPrompt.checked,
})

const plural = (count) => `${count} ${count === 1 ? 'entry' : 'entries'}`

async function copyText(text) {
  try {
    await navigator.clipboard.writeText(text)
  } catch {
    // Older web views refuse the clipboard API; a hidden text box still works.
    const box = document.createElement('textarea')
    box.value = text
    box.style.cssText = 'position:fixed;opacity:0;'
    document.body.appendChild(box)
    box.select()
    const copied = document.execCommand('copy')
    box.remove()
    if (!copied) throw new Error('The clipboard is not available')
  }
}

async function runExport(action) {
  exportCopy.disabled = exportSave.disabled = true
  exportMessage('')
  try {
    await action()
  } catch (error) {
    exportMessage(String(error?.message ?? error), 'error')
  } finally {
    exportCopy.disabled = exportSave.disabled = false
  }
}

exportCopy.addEventListener('click', () =>
  runExport(async () => {
    const { text, count } = await invoke('export_text', exportArgs())
    await copyText(text)
    exportMessage(`Copied ${plural(count)}. Paste them into your AI assistant.`, 'success')
  }),
)

exportSave.addEventListener('click', () =>
  runExport(async () => {
    const extension = EXTENSIONS[exportFormat.value]
    const path = await save({
      title: 'Save your entries',
      defaultPath: `hourly-${exportRange.value}.${extension}`,
      filters: [{ name: FORMAT_NAMES[exportFormat.value], extensions: [extension] }],
    })
    if (!path) return
    const count = await invoke('export_to_file', { path, ...exportArgs() })
    exportMessage(`Saved ${plural(count)} to ${path.split(/[\\/]/).pop()}.`, 'success')
  }),
)

// ---------- Footer ----------
$('help').addEventListener('click', () => invoke('open_help'))
$('openLog').addEventListener('click', () =>
  invoke('open_log').catch((error) => setStatus(String(error), 'error')),
)
$('formatDocs').addEventListener('click', () => invoke('open_external', { url: FORMAT_DOCS }))
$('minimizeBtn').addEventListener('click', () => appWindow.minimize())
$('closeBtn').addEventListener('click', closeWindow)
window.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') closeWindow()
})
window.addEventListener('focus', () => {
  if (!$('panel-general').hidden) refreshStats()
})

// ---------- Start ----------
async function load() {
  const [settings, list, welcome] = await Promise.all([
    invoke('get_settings'),
    invoke('template_presets'),
    invoke('take_welcome'),
  ])
  presets = list
  fillPresets()
  rootInput.value = settings.journalRoot || ''
  templateInput.value = settings.pathTemplate
  markerInput.value = settings.marker
  minLengthInput.value = settings.minLength
  lastSaved = JSON.stringify({
    journalRoot: settings.journalRoot,
    pathTemplate: settings.pathTemplate,
    marker: settings.marker,
    minLength: settings.minLength,
  })
  syncSelect()
  updateNotesVisibility()
  updatePreview()
  refreshStats()
  // On the very first launch the one decision worth making is the notes folder.
  if (welcome) showTab('notes')
}
load()
