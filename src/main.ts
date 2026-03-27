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
        <button id="save-shortcut" class="secondary">保存快捷键</button>
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
const saveShortcutButton = document.querySelector<HTMLButtonElement>('#save-shortcut');

function setStatus(message: string) {
  if (statusPill) {
    statusPill.textContent = message;
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
    shortcutValue.textContent = settings.capture_shortcut;
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

toggleAutostartButton?.addEventListener('click', async () => {
  const current = await invoke<AutostartStatus>('get_autostart_status');
  const next = !current.enabled;

  await invoke('set_autostart_enabled', { enabled: next });
  await refreshState();
});

saveShortcutButton?.addEventListener('click', async () => {
  const shortcut = shortcutInput?.value.trim();
  if (!shortcut) {
    setStatus('请输入快捷键');
    return;
  }

  setStatus('正在保存快捷键');
  await invoke<AppSettings>('update_capture_shortcut', { shortcut });
  await refreshState();
  setStatus('快捷键已更新');
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
