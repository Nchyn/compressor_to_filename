/* ============================================================
   账户转账指令生成器 - 核心逻辑（纯原生 JS，零依赖）
   ============================================================ */
'use strict';

/* ---------------- 常量 ---------------- */
const REMARK_LINE = '账户转账@小致(小致)'; // 固定备注
const ACCOUNT_MIN = 9;   // 账户ID最少位数
const ACCOUNT_MAX = 15;   // 账户ID最多位数

const state = {
  mode: 'multi_target', // multi_target | multi_source
  sources: [],
  targets: [],
  amounts: [],
  items: [],
  pairErrs: [],
};

const $ = (id) => document.getElementById(id);

/* ---------------- 工具函数 ---------------- */
function esc(s) {
  return String(s).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
}

// 全角数字 → 半角
function normalizeFullWidth(raw) {
  return raw.replace(/[０-９]/g, (c) => String.fromCharCode(c.charCodeAt(0) - 0xfee0));
}

function formatAmount(n) {
  return String(Math.round(n * 100) / 100);
}

// 千分位逗号分隔（仅显示用；指令复制文本保持原始无逗号金额）
function fmtNum(n) {
  const s = String(n);
  const dot = s.indexOf('.');
  const intPart = dot === -1 ? s : s.slice(0, dot);
  const decPart = dot === -1 ? '' : s.slice(dot);
  return intPart.replace(/\B(?=(\d{3})+(?!\d))/g, ',') + decPart;
}

/* ---------------- 亿级彩蛋 ---------------- */
// 预存 5 个国家（人民币计价、近似年 GDP），数字超亿级时按量级匹配
const GDP_COUNTRIES = [
  { min: 1e8,  name: '图瓦卢' },   // GDP ≈ 5 亿
  { min: 1e11, name: '卢森堡' },   // GDP ≈ 6300 亿
  { min: 1e12, name: '新加坡' },   // GDP ≈ 3.6 万亿
  { min: 1e13, name: '德国' },     // GDP ≈ 32 万亿
  { min: 1e14, name: '美国' },     // GDP ≈ 200 万亿
];
// 数字 ≥ 1 亿 → 「你要买下xx么」（xx 为量级匹配的国家）；否则 null
function gdpCountry(n) {
  let hit = null;
  for (const c of GDP_COUNTRIES) if (n >= c.min) hit = c;
  return hit ? `你要买下${hit.name}么` : null;
}

/* ---------------- 解析器 ---------------- */
// 账户ID：从任意混杂文本中提取 9-11 位数字
// 清洗规则：
//   9-15 位        → 有效账户
//   1-5 位          → 无关噪声（状态码/数量等），静默忽略并统计
//   6-8 位 / 12+ 位 → 疑似账户ID但位数不符，警告提示后忽略 
// 重复账户：默认保留；mergeDup 为 true 时合并
function parseAccounts(raw, mergeDup = false) {
  const text = normalizeFullWidth(raw);
  const values = [];
  const msgs = []; // { type: 'err' | 'warn', text }
  let noise = 0;
  const re = /\d+/g;
  let m;
  while ((m = re.exec(text)) !== null) {
    const s = m[0];
    const len = s.length;
    if (len >= ACCOUNT_MIN && len <= ACCOUNT_MAX) {
      values.push(s);
    } else if (len < 6) {
      noise++;
    } else {
      msgs.push({ type: 'warn', text: `第 ${m.index + 1} 位起「${len > 10 ? s.slice(0, 10) + '…' : s}」共 ${len} 位，非 ${ACCOUNT_MIN}–${ACCOUNT_MAX} 位，已忽略` });
    }
  }
  // 去重统计（是否合并由调用方决定）
  const seen = new Set();
  const dedup = [];
  for (const v of values) {
    if (!seen.has(v)) { seen.add(v); dedup.push(v); }
  }
  const dupCount = values.length - dedup.length;
  const merged = mergeDup ? dedup : values;
  if (dupCount > 0 && mergeDup) {
    msgs.push({ type: 'warn', text: `已合并 ${dupCount} 个重复账户` });
  }
  if (noise > 0) {
    msgs.push({ type: 'warn', text: `已忽略 ${noise} 个非账户数字（状态码/数量等小数字）` });
  }
  return { values: merged, dupCount, msgs };
}

// 金额：提取正数，最多 2 位小数
function parseAmounts(raw) {
  const text = normalizeFullWidth(raw);
  const values = [];
  const msgs = [];
  const re = /\d+(?:\.\d+)?/g;
  let m;
  while ((m = re.exec(text)) !== null) {
    const s = m[0];
    const n = Number(s);
    const dec = s.includes('.') ? s.split('.')[1] : '';
    if (n <= 0) {
      msgs.push({ type: 'err', text: `金额「${s}」必须大于 0` });
    } else if (dec.length > 2) {
      msgs.push({ type: 'err', text: `金额「${s}」最多保留 2 位小数` });
    } else {
      values.push(formatAmount(n));
    }
  }
  return { values, msgs };
}

/* ---------------- 配对生成 ---------------- */
function buildTransfers({ sources, targets, amounts, mode }) {
  const errors = [];
  const emptyChecks = [
    [sources.length === 0, '转出账户为空'],
    [targets.length === 0, '转入账户为空'],
    [amounts.length === 0, '转账金额为空'],
  ];
  for (const [bad, msg] of emptyChecks) if (bad) errors.push(msg);

  let items = [];
  if (mode === 'multi_target') {
    const N = targets.length;
    if (N > 0 && sources.length > 0 && amounts.length > 0) {
      if (![1, N].includes(amounts.length)) {
        errors.push(`金额数量(${amounts.length})须为 1（全部相同）或与目标数量(${N})一致`);
      }
      if (![1, N].includes(sources.length)) {
        errors.push(`转出账户数量(${sources.length})须为 1（全部使用）或与目标数量(${N})一致`);
      }
    }
    if (!errors.length) {
      items = targets.map((t, i) => ({
        source: sources.length === 1 ? sources[0] : sources[i],
        target: t,
        amount: amounts.length === 1 ? amounts[0] : amounts[i],
      }));
    }
  } else { // multi_source：多来源 → 单目标，全部来源账户逗号合并为一行
    if (sources.length > 0) {
      if (targets.length !== 1) {
        errors.push(`多来源转账模式下目标账户须为 1 个（当前 ${targets.length} 个）`);
      }
      if (amounts.length !== 1) {
        errors.push(`多来源转账模式下金额须为 1 个（当前 ${amounts.length} 个）`);
      }
    }
    if (!errors.length && sources.length > 0) {
      // 多来源合并为一行，指令金额 = 输入金额 × 转出账户数
      items = [{
        source: sources.join(','),
        target: targets[0],
        amount: formatAmount(Number(amounts[0]) * sources.length),
      }];
    }
  }
  return { items, errors };
}

/* ---------------- 指令格式化 ---------------- */
function formatInstruction(it) {
  return `来源账户：${it.source}\n目标账户：${it.target}\n转账金额(元)：${it.amount}\n${REMARK_LINE}`;
}

/* ---------------- 剪贴板 ---------------- */
async function copyText(text) {
  if (navigator.clipboard && window.isSecureContext) {
    try {
      await navigator.clipboard.writeText(text);
      return true;
    } catch (_) { /* 降级 */ }
  }
  const ta = document.createElement('textarea');
  ta.value = text;
  ta.style.position = 'fixed';
  ta.style.opacity = '0';
  document.body.appendChild(ta);
  ta.focus();
  ta.select();
  let ok = false;
  try { ok = document.execCommand('copy'); } catch (_) { /* ignore */ }
  ta.remove();
  return ok;
}

/* ---------------- 粘贴按钮三态 ---------------- */
// idle「粘贴数据」绿描边 / cooling 冷却 1s orange / ready「继续粘贴」绿实底
function setPasteBtn(btn, state) {
  btn.classList.remove('cooling', 'ready');
  if (state === 'cooling') {
    btn.classList.add('cooling');
    btn.textContent = '继续粘贴';
  } else if (state === 'ready') {
    btn.classList.add('ready');
    btn.textContent = '继续粘贴';
  } else {
    btn.textContent = '粘贴数据';
  }
}

/* ---------------- 渲染：统计徽标 ---------------- */
function renderChip(el, text, ok) {
  el.textContent = text;
  el.classList.toggle('ok', !!ok);
}

/* ---------------- 渲染：解析消息 ---------------- */
function renderMsgs(ul, msgs) {
  const frag = document.createDocumentFragment();
  for (const msg of msgs) {
    const li = document.createElement('li');
    li.className = msg.type;
    li.textContent = msg.text;
    frag.appendChild(li);
  }
  ul.replaceChildren(frag);
}

/* ---------------- 渲染：配对预览 ---------------- */
function renderPreview() {
  const box = $('pairPreview');
  if (!state.items.length) {
    box.innerHTML = '<div class="empty-tip">输入账户与金额后，这里将显示配对关系预览</div>';
    return;
  }
  const MAX = 60;
  const rows = state.items.slice(0, MAX);
  // 第一个超过亿级触发线的行（而非第一行）
  const eggIdx = rows.findIndex((it) => Number(it.amount) >= 1e8);
  let html = rows
    .map((it, idx) => {
      const same = it.source === it.target;
      // 首个超亿级的行 → 显示彩蛋文案
      const egg = idx === eggIdx ? gdpCountry(Number(it.amount)) : null;
      return (
        `<div class="row-wrap">` +
        `<div class="pair-row">` +
        `<span class="acc" title="${esc(it.source)}">${esc(it.source)}</span>` +
        `<span class="arrow">→</span>` +
        (same
          ? `<span class="acc acc-warn" title="${esc(it.target)}">账户相同</span>`
          : `<span class="acc" title="${esc(it.target)}">${esc(it.target)}</span>`) +
        (egg
          ? `<span class="amt">${esc(egg)}</span>`
          : `<span class="amt">¥${esc(fmtNum(it.amount))}</span>`) +
        `</div>` +
        `<button type="button" class="row-copy ripple" data-i="${idx}">复制</button>` +
        `</div>`
      );
    })
    .join('');
  if (state.items.length > MAX) html += `<div class="pair-row" style="color:var(--outline)">… 其余 ${state.items.length - MAX} 条略</div>`;
  box.innerHTML = html;
}

/* ---------------- 渲染：金额统计 ---------------- */
function renderStat() {
  const el = $('statLine');
  const items = state.items;
  if (!items.length) {
    el.textContent = '';
    return;
  }
  // 浮点累计后用 formatAmount 归整到 2 位小数，避免精度噪声（如 0.30000001）
  const total = formatAmount(items.reduce((s, it) => s + Number(it.amount), 0));
  el.innerHTML = `共 <b>${items.length}</b> 笔转账 · 合计金额 <b>¥${fmtNum(total)}</b>`;
}

/* ---------------- 渲染：指令结果 ---------------- */
function renderResult() {
  const box = $('resultText');
  const items = state.items;
  if (!items.length) {
    box.innerHTML = '<div class="empty-tip">原始指令预览</div>';
    renderChip($('resultChip'), '0 条', false);
    return;
  }
  // 浮点累计后用 formatAmount 归整到 2 位小数，避免精度噪声
  const total = formatAmount(items.reduce((s, it) => s + Number(it.amount), 0));
  // 合计超亿级 → 彩蛋文案
  const egg = gdpCountry(Number(total));
  renderChip($('resultChip'), egg || `${items.length} 条 · 合计 ¥${fmtNum(total)}`, true);

  let html = '';
  let ln = 0;
  items.forEach((it, idx) => {
    if (idx > 0) html += '<div class="gap-row"></div>';
    html +=
      `<div class="code-line"><span class="ln">${++ln}</span><span class="txt"><span class="key">来源账户：</span><span class="val">${esc(it.source)}</span></span></div>` +
      `<div class="code-line"><span class="ln">${++ln}</span><span class="txt"><span class="key">目标账户：</span><span class="val">${esc(it.target)}</span></span></div>` +
      `<div class="code-line"><span class="ln">${++ln}</span><span class="txt"><span class="key">转账金额(元)：</span><span class="amt-v">${esc(fmtNum(it.amount))}</span></span></div>` +
      `<div class="code-line"><span class="ln">${++ln}</span><span class="txt"><span class="remark">${esc(REMARK_LINE)}</span></span></div>`;
  });
  box.innerHTML = html;
}

/* ---------------- 模式描述（自动判定） ---------------- */
function renderModeDesc() {
  const isMultiSource = state.mode === 'multi_source';
  $('modeChip').textContent = isMultiSource ? '多来源转账' : '多目标转账';
  $('modeDesc').innerHTML = isMultiSource
    ? '<b>自动模式</b>：转出账户多对一合并 —— 转出 N 个（逗号合并为一行）→ 目标 1 个 · 指令金额 = 输入金额 × N'
    : '<b>自动模式</b>：目标账户为变量列 —— 目标 N 个 · 金额 1 或 N 个 · 转出 1 或 N 个，均一一对应';
}

/* ---------------- 重复账户复选框显隐 ---------------- */
function syncDupCb(prefix, dupCount) {
  const wrap = $(`${prefix}Dup`);
  if (dupCount > 0) {
    wrap.hidden = false;
    wrap.querySelector('span').textContent = `合并 ${dupCount} 个重复账户`;
  } else {
    wrap.hidden = true;
  }
}

/* ---------------- 主流程 ---------------- */
function refresh() {
  const src = parseAccounts($('sourceInput').value, $('sourceDedupCb').checked);
  const tgt = parseAccounts($('targetInput').value, $('targetDedupCb').checked);
  const amt = parseAmounts($('amountInput').value);

  state.sources = src.values;
  state.targets = tgt.values;
  state.amounts = amt.values;

  // 自动判断配对模式：多转出 + 单转入 → 多来源合并；其余 → 多目标
  state.mode = (state.sources.length > 1 && state.targets.length === 1) ? 'multi_source' : 'multi_target';

  const pair = buildTransfers({ sources: state.sources, targets: state.targets, amounts: state.amounts, mode: state.mode });
  state.items = pair.items;
  state.pairErrs = pair.errors;

  // 徽标
  renderChip($('sourceChip'), `${src.values.length} 个`, src.values.length > 0 && !src.msgs.some((m) => m.type === 'err'));
  renderChip($('targetChip'), `${tgt.values.length} 个`, tgt.values.length > 0 && !tgt.msgs.some((m) => m.type === 'err'));
  renderChip($('amountChip'), `${amt.values.length} 个`, amt.values.length > 0 && !amt.msgs.some((m) => m.type === 'err'));

  // 消息
  renderMsgs($('sourceMsgs'), src.msgs);
  renderMsgs($('targetMsgs'), tgt.msgs);
  renderMsgs($('amountMsgs'), amt.msgs);

  // 重复账户复选框显隐
  syncDupCb('source', src.dupCount);
  syncDupCb('target', tgt.dupCount);

  // 配对错误
  const ul = $('pairMsgs');
  const frag = document.createDocumentFragment();
  state.pairErrs.forEach((e) => {
    const li = document.createElement('li');
    li.className = 'err';
    li.textContent = e;
    frag.appendChild(li);
  });
  ul.replaceChildren(frag);

  // 配对错误 → 对应输入框红色边框；无错误 → focus 绿框（合规）
  // 三个窗口均为空（初始态）时不标注任何边框
  const errMap = { sourceInput: [], targetInput: [], amountInput: [] };
  for (const e of state.pairErrs) {
    if (e.includes('转出')) errMap.sourceInput.push(e);
    if (e.includes('转入') || e.includes('目标')) errMap.targetInput.push(e);
    if (e.includes('金额')) errMap.amountInput.push(e);
  }
  const hasAnyContent = ['sourceInput', 'targetInput', 'amountInput'].some((id) => $(id).value.trim());
  for (const [id, errs] of Object.entries(errMap)) {
    const ta = $(id);
    if (!hasAnyContent) { ta.classList.remove('err', 'ok'); continue; }
    ta.classList.toggle('err', errs.length > 0);
    ta.classList.toggle('ok', errs.length === 0);
  }

  renderModeDesc();
  renderPreview();
  renderStat();
  renderResult();

  // 同步粘贴按钮状态：输入清空 → 「粘贴数据」；已有内容 → 「继续粘贴」（冷却中不干预）
  document.querySelectorAll('.paste-btn').forEach((btn) => {
    if (btn.classList.contains('cooling')) return;
    const ta = $(btn.dataset.paste);
    setPasteBtn(btn, ta && ta.value.trim() ? 'ready' : 'idle');
  });
}

/* ---------------- 事件绑定 ---------------- */
function bindEvents() {
  ['sourceInput', 'targetInput', 'amountInput'].forEach((id) => {
    $(id).addEventListener('input', refresh);
  });

  ['sourceDedupCb', 'targetDedupCb'].forEach((id) => {
    $(id).addEventListener('change', refresh);
  });

  // 粘贴数据按钮：读取剪贴板追加到输入框末尾（换行分隔），按下进入 orange 冷却 1s
  document.querySelectorAll('.paste-btn').forEach((btn) => {
    btn.addEventListener('click', async () => {
      if (btn.classList.contains('cooling')) return; // 冷却中
      const ta = $(btn.dataset.paste);
      if (!ta) return;
      let text = '';
      if (navigator.clipboard && window.isSecureContext) {
        try { text = await navigator.clipboard.readText(); } catch (_) { /* 权限被拒 */ }
      }
      if (!text) {
        ta.focus();
        alert('无法自动读取剪贴板，请授权剪贴板权限或手动粘贴（Ctrl+V）');
        return;
      }
      // 追加模式：已有内容时换行续贴
      ta.value = ta.value.trim() ? ta.value.replace(/\s+$/, '') + '\n' + text : text;
      refresh();
      // 冷却 1s（orange），结束后按输入框内容决定就绪态
      setPasteBtn(btn, 'cooling');
      setTimeout(() => {
        setPasteBtn(btn, ta.value.trim() ? 'ready' : 'idle');
      }, 1000);
    });
  });

  // 单条复制：配对行右侧按钮（点击后保持绿色「已复制」，可重复点击）
  $('pairPreview').addEventListener('click', async (e) => {
    const btn = e.target.closest('.row-copy');
    if (!btn) return;
    const item = state.items[Number(btn.dataset.i)];
    if (!item) return;
    const ok = await copyText(formatInstruction(item));
    if (ok) {
      btn.classList.add('done');
      btn.textContent = '已复制';
      // 复制后标记
      const mk = $('copiedMark');
      mk.hidden = false;
      clearTimeout(mk._t);
      mk._t = setTimeout(() => { mk.hidden = true; }, 2500);
    } else {
      alert('复制失败，请手动复制该条指令');
    }
  });

  // 清空重置按钮：多状态机
  // 默认「清空重置」orange 幽灵 → 点击进入「确认清除？」实色红 → 确认后「已清除」实色红 → 自动回默认
  const resetBtn = $('resetBtn');
  let resetTimer = null;

  function setResetState(state) {
    resetBtn.classList.remove('confirm', 'cleared');
    clearTimeout(resetTimer);
    if (state === 'confirm') {
      resetBtn.classList.add('confirm');
      resetBtn.textContent = '确认清除？';
      resetTimer = setTimeout(() => setResetState('default'), 3000); // 3s 未确认自动回默认
    } else if (state === 'cleared') {
      resetBtn.classList.add('cleared');
      resetBtn.textContent = '已清除';
      resetTimer = setTimeout(() => setResetState('default'), 1500);
    } else {
      resetBtn.textContent = '清空重置';
    }
  }

  resetBtn.addEventListener('click', () => {
    if (resetBtn.classList.contains('confirm')) {
      // 二次点击确认：执行清除
      ['sourceInput', 'targetInput', 'amountInput'].forEach((id) => { $(id).value = ''; });
      $('sourceDedupCb').checked = false;
      $('targetDedupCb').checked = false;
      refresh();
      setResetState('cleared');
    } else {
      // 首次点击：进入等待确认状态
      setResetState('confirm');
    }
  });
}

/* ---------------- 启动 ---------------- */
document.addEventListener('DOMContentLoaded', () => {
  bindEvents();
  refresh();
});
