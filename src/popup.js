const { invoke } = window.__TAURI__.core
const { listen } = window.__TAURI__.event

// The minimum comes from the settings (50 unless the user changed it) and is
// refreshed together with the rest of the context.
let minLength = 50

// Characters as a person counts them: an emoji is one, not two.
const lengthOf = (text) => Array.from(text.trim()).length

const textarea = document.getElementById('note')
const saveButton = document.getElementById('save')
const skipButton = document.getElementById('skip')
const counter = document.getElementById('counter')

function pad(n) {
  return String(n).padStart(2, '0')
}

function updateContext() {
  invoke('get_context').then(({ lastEntryAt, lastEntryText, canSkip, minLength: min }) => {
    minLength = min
    updateState()
    // canSkip is calculated from today's date in the backend, so a popup that
    // stayed open overnight gets its "Skip" button back by itself.
    skipButton.hidden = !canSkip

    if (!lastEntryAt) {
      textarea.placeholder = 'First entry'
      return
    }
    const d = new Date(lastEntryAt)
    const now = new Date()
    const sameDay =
      d.getFullYear() === now.getFullYear() &&
      d.getMonth() === now.getMonth() &&
      d.getDate() === now.getDate()
    const time = `${pad(d.getHours())}:${pad(d.getMinutes())}`
    // The date is only shown for an entry from an earlier day.
    const when = sameDay ? time : `${pad(d.getDate())}.${pad(d.getMonth() + 1)} ${time}`
    const head = `Last entry: ${when}`
    textarea.placeholder = lastEntryText ? `${head}\n${lastEntryText}` : head
  })
}

function updateState() {
  const length = lengthOf(textarea.value)
  // Something must be written even when the minimum is 0.
  const ready = length > 0 && length >= minLength
  saveButton.disabled = !ready
  counter.textContent = minLength > 0 ? `${length} / ${minLength}` : `${length}`
  counter.classList.remove('error')
  counter.classList.toggle('ready', ready)
}

function showError(message) {
  counter.textContent = String(message)
  counter.classList.remove('ready')
  counter.classList.add('error')
}

function submit() {
  const text = textarea.value.trim()
  const length = lengthOf(text)
  if (length === 0 || length < minLength) return
  saveButton.disabled = true
  invoke('submit_note', { text }).catch((error) => {
    showError(error)
    saveButton.disabled = false
  })
}

updateContext()
// If the popup is already open when another trigger arrives, no second window
// is made: the backend sends this event instead so the placeholder stays fresh.
listen('refresh-context', updateContext)
// The window may hang around past midnight, when the right to skip renews.
window.addEventListener('focus', updateContext)
setInterval(updateContext, 60 * 1000)

textarea.addEventListener('input', updateState)
saveButton.addEventListener('click', submit)
skipButton.addEventListener('click', () => invoke('skip_note'))
textarea.addEventListener('keydown', (e) => {
  if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) submit()
})

updateState()
