const MAX_W = 8000;
const MAX_H = 8000;
const GAP = 15; // 图片间距
const TEXT_HEIGHT = 20; // 文字高度区域
const PREVIEW_MAX = 4000; // 网页显示最大尺寸
const MARGIN = 15; // 全局空白

const fileInput = document.querySelector('#file');
const pickBtn = document.querySelector('#pick');
const clearBtn = document.querySelector('#clear');
const dropZone = document.querySelector('#drop');
const out = document.querySelector('#out');
const statusEl = document.querySelector('#status');
const themeToggleBtn = document.querySelector('#theme-toggle');

// 默认设置深色模式
if(localStorage.getItem('theme') === 'dark'){
  document.body.classList.add('theme-dark');
  themeToggleBtn.textContent = '☀️';
}

pickBtn.addEventListener('click', () => fileInput.click());
clearBtn.addEventListener('click', () => { out.innerHTML = ''; status(''); });
fileInput.addEventListener('change', () => handleFiles(fileInput.files));

['dragenter', 'dragover'].forEach(evt => dropZone.addEventListener(evt, e => { e.preventDefault(); e.stopPropagation(); dropZone.classList.add('dragover'); }));
['dragleave', 'drop'].forEach(evt => dropZone.addEventListener(evt, e => { e.preventDefault(); e.stopPropagation(); dropZone.classList.remove('dragover'); }));
dropZone.addEventListener('drop', e => handleFiles(e.dataTransfer.files));

themeToggleBtn.addEventListener('click', () => {
  document.body.classList.toggle('theme-dark');
  const isDarkMode = document.body.classList.contains('theme-dark');
  themeToggleBtn.textContent = isDarkMode ? '☀️' : '🌙';
  localStorage.setItem('theme', isDarkMode ? 'dark' : 'light');
});

function status(txt) {
  statusEl.textContent = txt;
}

async function handleFiles(fileList) {
  out.innerHTML = '';
  if (!fileList || !fileList.length) {
    status('未选择文件。');
    return;
  }
  let files = Array.from(fileList).filter(f => f.type.startsWith('image/'));
  if (!files.length) {
    status('未找到图片文件。');
    return;
  }

  files.sort((a, b) =>
    a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: 'base' })
  );

  status(`正在读取 ${files.length} 张图片…`);
  const bitmaps = [];
  let i = 0;
  for (const f of files) {
    try {
      const bm = await loadBitmap(f);
      bitmaps.push({ bitmap: bm, name: f.name, w: bm.width, h: bm.height });
      status(`已载入 ${++i}/${files.length}：${f.name} (${bm.width}×${bm.height})`);
    } catch (err) {
      console.error(err);
      status(`读取失败：${f.name}`);
    }
  }
  if (!bitmaps.length) {
    status('读取失败或无有效图片。');
    return;
  }

  let layout = layoutSingleSheet(bitmaps, 1);
  
  const MAX_TOTAL_HEIGHT = MAX_H; // 纵向最大高度
  let scale = 1;
  let isScaled = false;

  // 纵向超限逐级缩放
  while (layout.height > MAX_TOTAL_HEIGHT) {
    isScaled = true;
    if (scale === 1) {
      scale = 0.75;
    } else if (scale === 0.75) {
      scale = 0.5;
    } else {
      status('拼接图高度超限，无法继续缩放。');
      break;
    }
    status(`拼接图高度超限，正在尝试缩放至 ${Math.round(scale * 100)}%...`);
    layout = layoutSingleSheet(bitmaps, scale);
  }

  const canvas = renderSheet(layout);

  const resultContainer = document.createElement('div');
  resultContainer.className = 'result-img-container';
  out.appendChild(resultContainer);

  const preview = renderPreview(canvas);
  const imgEl = document.createElement('img');
  imgEl.id = 'result-image';
  imgEl.src = preview.toDataURL('image/jpeg', 1);
  resultContainer.appendChild(imgEl);

  const infoSpan = document.createElement('span');
  infoSpan.className = 'hint';
  infoSpan.style.marginTop = '8px';
  infoSpan.textContent = `生成图片尺寸：${canvas.width} × ${canvas.height} px`;
  if(isScaled) {
    infoSpan.textContent += ` (已缩放至 ${Math.round(scale * 100)}%)`;
  }
  out.appendChild(infoSpan);

  const btn = document.createElement('button');
  btn.textContent = '下载 JPG';
  btn.className = 'btn btn-primary';
  btn.style.height ='5em'
  btn.style.marginTop = '16px';
  btn.onclick = () => downloadCanvas(canvas, files);
  out.appendChild(btn);

  status('完成，已生成拼接图。');
}

function loadBitmap(file) {
  return new Promise((resolve, reject) => {
    const url = URL.createObjectURL(file);
    const img = new Image();
    img.onload = () => {
      URL.revokeObjectURL(url);
      resolve({
        width: img.naturalWidth,
        height: img.naturalHeight,
        name: file.name,
        draw(ctx, x, y, w, h) { ctx.drawImage(img, x, y, w, h); }
      });
    };
    img.onerror = reject;
    img.src = url;
  });
}

// 修复后的布局函数
function layoutSingleSheet(items, scale) {
  const positions = [];
  let x = MARGIN, y = MARGIN, rowH = 0, sheetW = 0;
  for (const it of items) {
    const w = Math.round(it.w * scale);
    const h = Math.round(it.h * scale);
    // 如果是行首，x为MARGIN，否则为MARGIN+w+GAP
    if (x + w + MARGIN > MAX_W && x > MARGIN) {
      x = MARGIN;
      y += rowH + GAP;
      rowH = 0;
    }
    
    positions.push({ item: it, x, y, w, h });
    x += w + GAP;
    rowH = Math.max(rowH, h + TEXT_HEIGHT);
    sheetW = Math.max(sheetW, x - GAP + MARGIN);
  }
  const sheetH = y + rowH + MARGIN;
  return { width: sheetW, height: sheetH, positions };
}

function renderSheet(layout) {
  const canvas = document.createElement('canvas');
  canvas.width = layout.width;
  canvas.height = layout.height;
  const ctx = canvas.getContext('2d');
  ctx.fillStyle = document.body.classList.contains('theme-dark') ? '#1f1f1f' : '#eee';
  ctx.fillRect(0, 0, canvas.width, canvas.height);

  for (const p of layout.positions) {
    // 绘制白色填充矩形作为背景
    ctx.fillStyle = document.body.classList.contains('theme-dark') ? '#303030' : '#fff';
    ctx.fillRect(p.x, p.y, p.w, p.h + TEXT_HEIGHT);

    // 绘制文字
    ctx.font = '16px -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif';
    ctx.textBaseline = 'top';
    ctx.fillStyle = document.body.classList.contains('theme-dark') ? '#fff' : '#000000e0';
    ctx.fillText(p.item.name, p.x + 8, p.y + 3); // 边距

    // 绘制图片
    p.item.bitmap.draw(ctx, p.x, p.y + TEXT_HEIGHT, p.w, p.h);

    // 绘制描边
    ctx.strokeStyle = document.body.classList.contains('theme-dark') ? '#434343' : '#f0f0f0';
    ctx.lineWidth = 1;
    ctx.strokeRect(p.x, p.y, p.w, p.h + TEXT_HEIGHT);
  }
  return canvas;
}

function renderPreview(canvas) {
  const scale = Math.min(PREVIEW_MAX / canvas.width, PREVIEW_MAX / canvas.height, 1);
  if (scale === 1) return canvas;

  const preview = document.createElement('canvas');
  preview.width = Math.round(canvas.width * scale);
  preview.height = Math.round(canvas.height * scale);
  const ctx = preview.getContext('2d');
  ctx.imageSmoothingQuality = 'high';
  ctx.drawImage(canvas, 0, 0, preview.width, preview.height);
  return preview;
}

function downloadCanvas(canvas, files) {
  canvas.toBlob(blob => {
    const a = document.createElement('a');
    const firstName = files[0]?.name?.replace(/\.[^.]+$/,'') || 'output';
    const fileName = `${firstName}_preview.jpg`;
    a.href = URL.createObjectURL(blob);
    a.download = fileName;
    a.click();
    URL.revokeObjectURL(a.href);
  }, 'image/jpeg', 0.9);
}
