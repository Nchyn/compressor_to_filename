const $=id=>document.getElementById(id);
let token=new URLSearchParams(location.hash.slice(1)).get('token');
try{token ||= sessionStorage.getItem('jpeg-token');if(token)sessionStorage.setItem('jpeg-token',token);}catch{}
if(location.hash)history.replaceState(null,'',location.pathname);
let connected=false,running=false,active=null,queue=[],urls=[];
async function api(path,body,type='application/json',signal){
 const response=await fetch(path,{method:'POST',headers:{Authorization:'Bearer '+token,'Content-Type':type},body:type==='application/json'?JSON.stringify(body):body,signal});
 const result=await response.json();if(!response.ok)throw Error(result.error||`HTTP ${response.status}`);return result;
}
async function checkConnection(){const controller=new AbortController(),timer=setTimeout(()=>controller.abort(),5000);try{await api('/api/health',{},'application/json',controller.signal);connected=true;$('connection').textContent='macOS客户端已连接 · 文件URL剪贴板';}catch(e){$('connection').textContent='客户端未连接：仍可编码和下载';$('notice').textContent='请通过start.command打开的专属网页连接。'+e.message;}finally{clearTimeout(timer);}}
const btn=(text,fn)=>{const b=document.createElement('button');b.textContent=text;b.onclick=fn;return b;};
function encode(file,options){return new Promise((resolve,reject)=>{
 const worker=new Worker(new URL('./jpeg-encode-worker.mjs',import.meta.url),{type:'module'}),start=performance.now();
 const clock=setInterval(()=>$('time').textContent=((performance.now()-start)/1000).toFixed(1)+'秒',100);
 const timer=setTimeout(()=>stop(Error('编码超时，Worker已终止')),180000);
 function cleanup(){clearTimeout(timer);clearInterval(clock);worker.terminate();active=null;$('cancel').disabled=true;}
 function stop(e){cleanup();reject(e);}
 active=()=>stop(Error('已取消'));$('cancel').disabled=false;
 worker.onmessage=({data})=>{if(data.type==='stage')$('phase').textContent=data.text;else{cleanup();data.type==='error'?reject(Error(data.message)):resolve(data.result);}};
 worker.onerror=e=>stop(Error(e.message));worker.postMessage({file,options});
});}
async function task(file,options){
 const article=document.createElement('article'),title=document.createElement('strong'),details=document.createElement('pre'),actions=document.createElement('div'),status=document.createElement('p');title.textContent=file.name||'粘贴图片';article.append(title,details,actions,status);$('results').prepend(article);
 const endToEnd=performance.now();
 try{
 const r=await encode(file,options);if(r.blob.size>=20000000)throw Error('最终字节校验失败');
 const source=URL.createObjectURL(file),out=URL.createObjectURL(r.blob);urls.push(source,out);
 details.textContent=`${r.origWidth}×${r.origHeight} → ${r.width}×${r.height}；${r.half?'已减半（有损）':'未缩放'}\nJPEG ${r.blob.size.toLocaleString()}字节；编码管道 ${r.ms}ms；解码/绘制 ${r.decodeDrawMs}ms\n各轮：${JSON.stringify(r.attempts)}`;
 let fileId=null,copyBusy=false;
 async function copyFile(){if(copyBusy)return;copyBusy=true;const controller=new AbortController(),start=performance.now();const timeout=setTimeout(()=>controller.abort(),90000);active=()=>controller.abort();$('cancel').disabled=false;
  try{status.textContent='上传最终JPEG到本机客户端…';if(!fileId){const uploaded=await api('/api/files',r.blob,'image/jpeg',controller.signal);fileId=uploaded.fileId;}
   status.textContent='正在写入文件剪贴板…';const report=await api('/api/clipboard/file',{fileId},'application/json',controller.signal);
   if(!report.written||!report.verified)throw Error('客户端未确认文件URL回读，请重启客户端');
   status.textContent=`JPEG文件已写入并回读验证：${report.name}；上传/复制 ${Math.round(performance.now()-start)}ms；距本次任务开始 ${Math.round(performance.now()-endToEnd)}ms。请在目标应用粘贴（不自动发送）。`;
  }catch(e){status.textContent='文件复制未确认：'+e.message+'。可重试或下载；取消时服务端可能已完成写入。';}
  finally{clearTimeout(timeout);copyBusy=false;active=null;$('cancel').disabled=true;}
 }
 const copyButton=btn('复制JPEG文件',copyFile);copyButton.disabled=!connected;
 actions.append(copyButton,btn('下载JPEG',()=>{const a=document.createElement('a');a.href=out;a.download=(file.name||'clipboard').replace(/\.[^.]+$/,'')+'-compressed.jpg';a.click();}),btn('原图/结果',()=>{$('original').src=source;$('output').src=out;$('preview').showModal();}));
 status.textContent=connected?'JPEG已生成，尚未写入；请点击“复制JPEG文件”':'JPEG已生成，但客户端未连接，无法写入文件剪贴板；请从start.command重新打开网页';if(options.autoCopy&&connected)await copyFile();
 }catch(e){status.textContent='未完成：'+e.message;}
}
async function pump(){if(running)return;running=true;$('clear').disabled=true;try{while(queue.length){const job=queue.shift();await task(job.file,job.options);}}finally{running=false;$('clear').disabled=false;$('phase').textContent='任务结束';}}
function enqueue(files){const accepted=Array.from(files).filter(f=>f.type.startsWith('image/'));if(!accepted.length){$('notice').textContent='未检测到图片';return;}const options={preHalf:$('preHalf').checked,qualityFirst:$('qualityFirst').checked,autoCopy:$('autoCopy').checked,background:$('background').value};queue.push(...accepted.map(file=>({file,options:{...options}})));$('notice').textContent=`已接收${accepted.length}张，串行处理。自动复制多张时剪贴板只保留最后一次写入的文件。`;pump();}
$('choose').onclick=()=>$('files').click();$('files').onchange=e=>{enqueue(e.target.files);e.target.value='';};
document.addEventListener('paste',e=>{
 const dt=e.clipboardData;
 let files=Array.from(dt?.files||[]).filter(f=>f.type.startsWith('image/'));
 if(!files.length)files=Array.from(dt?.items||[]).filter(i=>i.kind==='file'&&i.type.startsWith('image/')).map(i=>i.getAsFile()).filter(Boolean);
 if(files.length){e.preventDefault();$('phase').textContent='已捕获粘贴图片，准备处理…';enqueue(files);}
 else $('notice').textContent='已收到粘贴事件，但未检测到图片。若复制的是Finder文件或本工具输出的JPEG文件引用，请拖入/选择文件；截图请用⌘⇧⌃4复制到剪贴板。';
});
let readingClipboard=false;
async function readClipboard(){
 if(readingClipboard)return;readingClipboard=true;$('notice').textContent='正在读取剪贴板，请允许浏览器权限…';
 try{
  if(!navigator.clipboard?.read)throw Error('当前浏览器不支持主动读取；请用⌘V、拖入或选择文件');
  const items=await navigator.clipboard.read(),files=[];
  for(const item of items){const type=item.types.find(t=>t.startsWith('image/'));if(type){const blob=await item.getType(type);files.push(new File([blob],'clipboard.'+(type==='image/jpeg'?'jpg':'png'),{type}));}}
  if(!files.length)throw Error('剪贴板没有网页可读取的图片。文件引用请拖入/选择，或重新复制截图');
  enqueue(files);
 }catch(e){$('notice').textContent='未读取到图片：'+e.message;}
 finally{readingClipboard=false;}
}
$('readClipboard').onclick=readClipboard;
document.addEventListener('keydown',e=>{if(e.ctrlKey&&!e.metaKey&&!e.altKey&&!e.shiftKey&&e.code==='KeyV'&&!e.repeat){e.preventDefault();readClipboard();}});
$('drop').ondragover=e=>e.preventDefault();$('drop').ondrop=e=>{e.preventDefault();enqueue(e.dataTransfer.files);};$('cancel').onclick=()=>active?.();
$('clear').onclick=()=>{urls.forEach(u=>URL.revokeObjectURL(u));urls=[];$('results').replaceChildren();$('original').removeAttribute('src');$('output').removeAttribute('src');};$('close').onclick=()=>$('preview').close();$('native').onclick=()=>$('images').classList.toggle('native');
window.addEventListener('pagehide',()=>{active?.();urls.forEach(u=>URL.revokeObjectURL(u));});
// Bind paste/file controls first; a slow client must never prevent image ingestion.
checkConnection();
