import './styles.css';

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

type CaptureRecord = {
  id: string;
  created_at: string;
  width: number;
  height: number;
  path: string;
};

type AutostartStatus = {
  enabled: boolean;
  supported: boolean;
};

type AppSettings = {
  capture_shortcut: string;
};

const app = document.querySelector<HTMLDivElement>('#app');

if (!app) {
  throw new Error('App root not found');
}

app.innerHTML = `
  <main class="shell">
    <section class="hero">
      <div>
        <p class="eyebrow">Flick</p>
        <h1>截图、剪贴板、OCR/翻译扩展骨架</h1>
        <p class="summary">
          全局快捷键触发区域截图。当前版本已经接入托盘、开机启动、剪贴板复制，并预留 OCR 与翻译模块接口。
        </p>
      </div>
      <div class="actions">
        <button id="capture-button" class="primary">开始截图</button>
        <button id="toggle-autostart" class="secondary">切换开机启动</button>
      </div>
    </section>

    <section class="panel">
      <div class="panel-header">
        <h2>截图设置</h2>
        <span id="status-pill" class="pill">初始化中</span>
      </div>
      <div class="shortcut-editor">
        <label class="field">
          <span>全局快捷键</span>
          <input id="shortcut-input" type="text" placeholder="CommandOrControl+Alt+A" />
        </label>
        <button id="record-shortcut" class="secondary">录制快捷键</button>
      </div>
      <dl class="status-grid">
        <div>
          <dt>截图快捷键</dt>
          <dd id="shortcut-value">CommandOrControl+Alt+A</dd>
        </div>
        <div>
          <dt>开机启动</dt>
          <dd id="autostart-value">检测中</dd>
        </div>
        <div>
          <dt>OCR</dt>
          <dd>Mock provider</dd>
        </div>
        <div>
          <dt>翻译</dt>
          <dd>Mock provider</dd>
        </div>
      </dl>
    </section>

    <section class="panel">
      <div class="panel-header">
        <h2>最近截图</h2>
        <span id="history-count" class="pill muted">0</span>
      </div>
      <div id="history" class="history empty">还没有截图记录</div>
    </section>
  </main>
`;

const captureButton = document.querySelector<HTMLButtonElement>('#capture-button');
const toggleAutostartButton = document.querySelector<HTMLButtonElement>('#toggle-autostart');
const statusPill = document.querySelector<HTMLSpanElement>('#status-pill');
const history = document.querySelector<HTMLDivElement>('#history');
const historyCount = document.querySelector<HTMLSpanElement>('#history-count');
const autostartValue = document.querySelector<HTMLDivElement>('#autostart-value');
const shortcutValue = document.querySelector<HTMLDivElement>('#shortcut-value');
const shortcutInput = document.querySelector<HTMLInputElement>('#shortcut-input');
const recordShortcutButton = document.querySelector<HTMLButtonElement>('#record-shortcut');
let isRecordingShortcut = false;

function setStatus(message: string) {
  if (statusPill) {
    statusPill.textContent = message;
  }
}

function formatShortcutLabel(shortcut: string) {
  return shortcut
    .replaceAll('CommandOrControl', 'Command/Ctrl')
    .replaceAll('Command', 'Command')
    .replaceAll('Control', 'Ctrl')
    .replaceAll('Alt', 'Option/Alt');
}

function shortcutFromKeyboardEvent(event: KeyboardEvent) {
  const parts: string[] = [];

  if (event.metaKey) {
    parts.push('Command');
  }
  if (event.ctrlKey) {
    parts.push('Control');
  }
  if (event.altKey) {
    parts.push('Alt');
  }
  if (event.shiftKey) {
    parts.push('Shift');
  }

  const code = event.code;
  let key: string | null = null;

  if (code.startsWith('Key')) {
    key = code.slice(3).toUpperCase();
  } else if (code.startsWith('Digit')) {
    key = code.slice(5);
  } else if (/^F\d{1,2}$/.test(code)) {
    key = code;
  } else {
    const codeMap: Record<string, string> = {
      Backquote: '`',
      Minus: '-',
      Equal: '=',
      BracketLeft: '[',
      BracketRight: ']',
      Backslash: '\\',
      Semicolon: ';',
      Quote: "'",
      Comma: ',',
      Period: '.',
      Slash: '/',
      Space: 'Space',
      Tab: 'Tab',
      Insert: 'Insert',
      Delete: 'Delete',
      Home: 'Home',
      End: 'End',
      PageUp: 'PageUp',
      PageDown: 'PageDown',
      ArrowUp: 'Up',
      ArrowDown: 'Down',
      ArrowLeft: 'Left',
      ArrowRight: 'Right'
    };
    key = codeMap[code] ?? null;
  }

  if (!key || parts.length === 0) {
    return null;
  }

  return [...parts, key].join('+');
}

function setRecordingShortcut(recording: boolean) {
  isRecordingShortcut = recording;
  if (recordShortcutButton) {
    recordShortcutButton.textContent = recording ? '按下快捷键…' : '录制快捷键';
  }
  if (shortcutInput) {
    shortcutInput.readOnly = recording;
  }
}

async function applyShortcut(shortcut: string) {
  try {
    console.log('[shortcut] apply request', { shortcut });
    setStatus('正在应用快捷键…');
    const settings = await invoke<AppSettings>('update_capture_shortcut', { shortcut });
    console.log('[shortcut] apply success', settings);

    if (shortcutInput) {
      shortcutInput.value = settings.capture_shortcut;
    }
    if (shortcutValue) {
      shortcutValue.textContent = formatShortcutLabel(settings.capture_shortcut);
    }

    setStatus(`快捷键已更新为 ${formatShortcutLabel(settings.capture_shortcut)}`);
  } catch (error: unknown) {
    const message = String(error);
    console.error('[shortcut] apply failed', error);
    if (message.includes('already registered') || message.includes('already in use') || message.includes('已被其他应用占用')) {
      setStatus('快捷键已被其他应用占用');
      return;
    }
    setStatus(`快捷键更新失败: ${message}`);
  }
}

function renderHistory(items: CaptureRecord[]) {
  if (!history || !historyCount) {
    return;
  }

  historyCount.textContent = String(items.length);

  if (items.length === 0) {
    history.className = 'history empty';
    history.textContent = '还没有截图记录';
    return;
  }

  history.className = 'history';
  history.innerHTML = items
    .map(
      (item) => `
        <article class="history-item">
          <div>
            <h3>${new Date(item.created_at).toLocaleString()}</h3>
            <p>${item.width} × ${item.height}</p>
          </div>
          <code>${item.path}</code>
        </article>
      `
    )
    .join('');
}

async function refreshState() {
  const [records, autostart, settings] = await Promise.all([
    invoke<CaptureRecord[]>('list_capture_history'),
    invoke<AutostartStatus>('get_autostart_status'),
    invoke<AppSettings>('get_app_settings')
  ]);

  renderHistory(records);

  if (autostartValue) {
    autostartValue.textContent = autostart.enabled ? '已启用' : autostart.supported ? '未启用' : '当前平台未支持';
  }

  if (shortcutValue) {
    shortcutValue.textContent = formatShortcutLabel(settings.capture_shortcut);
  }

  if (shortcutInput) {
    shortcutInput.value = settings.capture_shortcut;
  }

  setStatus('待命');
}

captureButton?.addEventListener('click', async () => {
  setStatus('准备截图');
  await invoke('start_capture');
});

recordShortcutButton?.addEventListener('click', () => {
  setRecordingShortcut(!isRecordingShortcut);
  console.log('[shortcut] record toggle', { recording: !isRecordingShortcut });
  setStatus(isRecordingShortcut ? '请按下新的快捷键' : '已取消录制快捷键');
});

shortcutInput?.addEventListener('keydown', (event) => {
  if (event.key !== 'Enter' || isRecordingShortcut) {
    return;
  }

  event.preventDefault();
  const shortcut = shortcutInput.value.trim();
  if (!shortcut) {
    setStatus('请输入快捷键');
    return;
  }
  void applyShortcut(shortcut);
});

shortcutInput?.addEventListener('blur', () => {
  if (isRecordingShortcut) {
    return;
  }

  const shortcut = shortcutInput.value.trim();
  if (!shortcut) {
    return;
  }

  void applyShortcut(shortcut);
});

toggleAutostartButton?.addEventListener('click', async () => {
  const current = await invoke<AutostartStatus>('get_autostart_status');
  const next = !current.enabled;

  await invoke('set_autostart_enabled', { enabled: next });
  await refreshState();
});

window.addEventListener('keydown', (event) => {
  if (!isRecordingShortcut) {
    return;
  }

  event.preventDefault();
  event.stopPropagation();

  if (event.key === 'Escape') {
    setRecordingShortcut(false);
    setStatus('已取消录制快捷键');
    return;
  }

  const shortcut = shortcutFromKeyboardEvent(event);
  if (!shortcut || !shortcutInput) {
    console.warn('[shortcut] invalid recording event', {
      key: event.key,
      code: event.code,
      metaKey: event.metaKey,
      ctrlKey: event.ctrlKey,
      altKey: event.altKey,
      shiftKey: event.shiftKey
    });
    setStatus('快捷键至少需要一个修饰键');
    return;
  }

  console.log('[shortcut] recorded', { shortcut });
  shortcutInput.value = shortcut;
  setRecordingShortcut(false);
  void applyShortcut(shortcut);
});

void listen('capture-finished', async () => {
  setStatus('截图完成，已复制到剪贴板');
  await refreshState();
});

void listen('capture-cancelled', () => {
  setStatus('截图已取消');
});

void listen('capture-error', (event) => {
  setStatus(`截图失败: ${String(event.payload)}`);
});

void refreshState().catch((error: unknown) => {
  setStatus(`初始化失败: ${String(error)}`);
});
