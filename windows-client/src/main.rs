#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{collections::{HashMap, HashSet}, ffi::OsStr, fs::{self, File, OpenOptions}, io::{Read, Write}, mem::{size_of, zeroed}, os::windows::{ffi::{OsStrExt, OsStringExt}, fs::MetadataExt}, path::{Path, PathBuf}, ptr::{null, null_mut}, sync::{atomic::{AtomicBool, AtomicU64, Ordering}, mpsc, Arc, Mutex}, thread, time::Duration};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};
use windows_sys::Win32::{Foundation::*, Graphics::{Gdi::*, Dwm::*}, System::{ApplicationInstallationAndServicing::*, Com::*, Registry::*, DataExchange::*, LibraryLoader::*, Memory::*}, UI::{Controls::*, Shell::*, WindowsAndMessaging::*, Input::KeyboardAndMouse::*}};

const DEFAULT_URL: &str = "https://cpr.adwetec.nchyn.net";
const MAX_CACHE: u64 = 1_000_000_000;
const MAX_ZIP: u64 = 250_000_000;
const CF_HDROP_FORMAT: u32 = 15;
const WM_LOG: u32 = WM_APP + 1;
const URL_ID: usize = 100;
const LOG_ID: usize = 101;
const SAVE: usize = 200;
const OPEN: usize = 201;
const START: usize = 202;
const STOP: usize = 203;
const CACHE: usize = 204;
const CLEAR: usize = 205;
const CLEAR_ORIGIN: usize = 206;
const LOCAL_MODE:usize=207;
const CUSTOM_MODE:usize=208;

fn now_seconds()->u64 {std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs()}
fn wide(s: impl AsRef<OsStr>) -> Vec<u16> { s.as_ref().encode_wide().chain(Some(0)).collect() }
fn fail(s: impl ToString) -> String { s.to_string() }
fn io<T>(r: std::io::Result<T>) -> Result<T,String> { r.map_err(fail) }

#[derive(Clone)]
struct Logger { queue: Arc<Mutex<Vec<String>>>, hwnd: isize }
impl Logger {
    fn log(&self, s: impl Into<String>) {
        let mut q = self.queue.lock().unwrap();
        if q.len() < 1000 { q.push(s.into()); }
        drop(q);
        unsafe { PostMessageW(self.hwnd as HWND, WM_LOG, 0, 0); }
    }
}

// All clipboard API calls (including cache protection queries) run on this STA.
enum ClipCommand { Copy(PathBuf, mpsc::Sender<Result<(),String>>), Paths(mpsc::Sender<Result<Vec<PathBuf>,String>>) }
#[derive(Clone)]
struct Clipboard(mpsc::Sender<ClipCommand>);
impl Clipboard {
    fn spawn() -> Result<Self,String> {
        let (tx, rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        thread::spawn(move || unsafe {
            let hr = CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32);
            if hr < 0 { let _ = ready_tx.send(Err(format!("STA initialization failed: {hr}"))); return; }
            let owner = CreateWindowExW(0, wide("STATIC").as_ptr(), wide("Clipboard owner").as_ptr(), 0, 0, 0, 0, 0, null_mut(), null_mut(), GetModuleHandleW(null()), null());
            if owner.is_null() { let _ = ready_tx.send(Err("Cannot create clipboard owner window".into())); CoUninitialize(); return; }
            let _ = ready_tx.send(Ok(()));
            loop {
                let mut msg: MSG = zeroed();
                while PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) != 0 { TranslateMessage(&msg); DispatchMessageW(&msg); }
                match rx.recv_timeout(Duration::from_millis(20)) {
                    Ok(ClipCommand::Copy(path, reply)) => { let _ = reply.send(copy_file(owner, &path)); }
                    Ok(ClipCommand::Paths(reply)) => { let _ = reply.send(with_clipboard(owner, || clipboard_paths())); }
                    Err(mpsc::RecvTimeoutError::Timeout) => (),
                    Err(_) => break,
                }
            }
            DestroyWindow(owner); CoUninitialize();
        });
        ready_rx.recv().map_err(fail)??;
        Ok(Self(tx))
    }
    fn copy(&self, path: PathBuf) -> Result<(),String> {
        let (tx,rx)=mpsc::channel(); self.0.send(ClipCommand::Copy(path,tx)).map_err(fail)?;
        rx.recv_timeout(Duration::from_secs(15)).map_err(fail)?
    }
    fn paths(&self) -> Result<Vec<PathBuf>,String> {
        let (tx,rx)=mpsc::channel(); self.0.send(ClipCommand::Paths(tx)).map_err(fail)?;
        rx.recv_timeout(Duration::from_secs(15)).map_err(fail)?
    }
}
unsafe fn with_clipboard<T>(owner: HWND, operation: impl FnOnce() -> Result<T,String>) -> Result<T,String> {
    let mut opened = false;
    for _ in 0..40 {
        if OpenClipboard(owner) != 0 { opened=true; break; }
        thread::sleep(Duration::from_millis(25));
    }
    if !opened { return Err("Clipboard busy; retry copying".into()); }
    let result=operation(); CloseClipboard(); result
}
unsafe fn clipboard_paths() -> Result<Vec<PathBuf>,String> {
    if IsClipboardFormatAvailable(CF_HDROP_FORMAT)==0 { return Ok(vec![]); }
    let data=GetClipboardData(CF_HDROP_FORMAT);
    if data.is_null() { return Err("Cannot read CF_HDROP".into()); }
    let count=DragQueryFileW(data, u32::MAX, null_mut(), 0);
    if count>10000 { return Err("Unexpected clipboard file count; refusing cache cleanup".into()); }
    let mut out=Vec::new();
    for index in 0..count {
        let len=DragQueryFileW(data,index,null_mut(),0);
        if len>32767 { return Err("Unexpected clipboard path length".into()); }
        let mut buf=vec![0u16;len as usize+1];
        if DragQueryFileW(data,index,buf.as_mut_ptr(),buf.len() as u32)!=len { return Err("Clipboard path read failed".into()); }
        out.push(PathBuf::from(std::ffi::OsString::from_wide(&buf[..len as usize])));
    }
    Ok(out)
}
unsafe fn copy_file(owner: HWND, path: &Path) -> Result<(),String> {
    if !path.is_file() { return Err("Cached file no longer exists".into()); }
    let mut encoded=wide(path); encoded.push(0); // double NUL list termination
    let bytes=size_of::<DROPFILES>()+encoded.len()*2;
    let memory=GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, bytes);
    if memory.is_null() { return Err("Clipboard allocation failed".into()); }
    let locked=GlobalLock(memory);
    if locked.is_null() { GlobalFree(memory); return Err("Clipboard memory lock failed".into()); }
    let dropfiles=locked as *mut DROPFILES;
    (*dropfiles).pFiles=size_of::<DROPFILES>() as u32;
    (*dropfiles).fWide=1;
    std::ptr::copy_nonoverlapping(encoded.as_ptr(), (locked as *mut u8).add(size_of::<DROPFILES>()) as *mut u16, encoded.len());
    GlobalUnlock(memory);
    let mut transferred=false;
    let result=with_clipboard(owner, || {
        if EmptyClipboard()==0 { return Err("Cannot empty clipboard".into()); }
        if SetClipboardData(CF_HDROP_FORMAT,memory).is_null() { return Err("Cannot set CF_HDROP".into()); }
        transferred=true; // Windows owns HGLOBAL from here, even if verification fails.
        Ok(())
    });
    if !transferred { GlobalFree(memory); }
    result?;
    // Reopen and read back the OS clipboard; success is never inferred from SetClipboardData alone.
    with_clipboard(owner, || {
        let paths=clipboard_paths()?;
        if paths.len()!=1 || paths[0]!=path { return Err("CF_HDROP read-back verification failed".into()); }
        Ok(())
    })
}

fn validated_url(text: &str) -> Result<(String,String),String> {
    let url=url::Url::parse(text.trim()).map_err(|_| "Invalid compressor URL".to_owned())?;
    if !url.username().is_empty() || url.password().is_some() || url.host_str().is_none() { return Err("URL must have a host and no credentials".into()); }
    if url.scheme()!="https" && !(url.scheme()=="http" && matches!(url.host_str(),Some("localhost"|"127.0.0.1"))) { return Err("Use HTTPS, or HTTP on localhost / 127.0.0.1".into()); }
    Ok((url.to_string(),url.origin().ascii_serialization()))
}
fn key() -> Result<String,String> { let mut bytes=[0;16]; getrandom::getrandom(&mut bytes).map_err(fail)?; Ok(bytes.iter().map(|b|format!("{b:02x}")).collect()) }
fn verify_cache_directory(root: &Path) -> Result<(),String> {
    // Reject junctions, symlinks and other reparse points, including cache ancestors.
    for ancestor in root.ancestors() {
        let metadata=io(fs::symlink_metadata(ancestor))?;
        if metadata.file_attributes() & 0x400 != 0 { return Err("Cache path contains a reparse point; cleanup/upload refused".into()); }
    }
    Ok(())
}
fn cache_size(root: &Path) -> Result<u64,String> {
    verify_cache_directory(root)?;
    let mut total=0u64;
    for entry in io(fs::read_dir(root))? {
        let entry=io(entry)?; let kind=io(entry.file_type())?;
        if io(fs::symlink_metadata(entry.path()))?.file_attributes() & 0x400 != 0 { return Err("Unexpected reparse point in cache; inspect cache manually".into()); }
        if kind.is_file() { total=total.checked_add(io(entry.metadata())?.len()).ok_or("Cache size overflow")?; }
        else if kind.is_dir() { total=total.checked_add(cache_size(&entry.path())?).ok_or("Cache size overflow")?; }
    }
    Ok(total)
}
fn safe_zip_name(path: &Path) -> Result<String,String> {
    let file=io(File::open(path))?;
    let mut archive=zip::ZipArchive::new(file).map_err(fail)?;
    if archive.len()>10000 { return Err("ZIP entry count limit is 10,000".into()); }
    let mut expanded=0u64; let mut roots=HashSet::new(); let mut names=HashSet::new();
    for i in 0..archive.len() {
        let entry=archive.by_index_raw(i).map_err(fail)?;
        let name=entry.name();
        expanded=expanded.checked_add(entry.size()).ok_or("ZIP size overflow")?;
        if expanded>MAX_ZIP { return Err("ZIP expanded size limit is 250MB".into()); }
        if entry.encrypted() || name.is_empty() || name.starts_with('/') || name.contains('\\') || name.contains('\0') || name.split('/').any(|p|p=="..") || name.as_bytes().get(1)==Some(&b':') || !names.insert(name.to_owned()) { return Err("Unsafe ZIP entry".into()); }
        roots.insert(name.split('/').next().unwrap().to_owned());
    }
    let root=if roots.len()==1 { roots.into_iter().next().unwrap() } else { "文件夹".into() };
    let name:String=root.chars().map(|c|if c.is_control() || "<>:\"/\\|?*".contains(c) {'_'}else{c}).take(100).collect();
    let name=name.trim_matches(['.',' ']);
    let stem=name.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved=matches!(stem.as_str(),"CON"|"PRN"|"AUX"|"NUL") || (stem.len()==4 && (stem.starts_with("COM")||stem.starts_with("LPT")) && matches!(stem.as_bytes()[3],b'1'..=b'9'));
    Ok(format!("{}.zip",if name.is_empty()||reserved {"文件夹"}else{name}))
}

struct Shared { last_health: AtomicU64, cache: PathBuf, files: Mutex<HashMap<String,PathBuf>>, cache_lock: Mutex<()>, clipboard: Clipboard, logger: Logger }
struct Service { stop: Arc<AtomicBool>, server: Arc<Server>, thread: thread::JoinHandle<()>, port: u16 }
impl Service {
    fn start(mut origin: String, shared: Arc<Shared>) -> Result<Self,String> {
        shared.last_health.store(0,Ordering::Relaxed);
        let mut bound=None;
        for port in 48731..=48735 {
            match Server::http(("127.0.0.1",port)) { Ok(server)=>{bound=Some((server,port));break;},Err(err)=>shared.logger.log(format!("Port {port} unavailable: {err}")) }
        }
        let (server,port)=bound.ok_or("Ports 48731–48735 unavailable; close duplicate clients")?;
        if origin.is_empty(){origin=format!("http://127.0.0.1:{port}");}
        shared.files.lock().unwrap().clear(); // Like the Mac process: IDs are scoped to one service lifetime.
        let server=Arc::new(server); let stop=Arc::new(AtomicBool::new(false));
        let (worker_server,worker_stop)=(server.clone(),stop.clone());
        let thread=thread::spawn(move || {
            shared.logger.log(format!("服务运行中 · http://127.0.0.1:{port} · 允许来源：{origin}"));
            while !worker_stop.load(Ordering::SeqCst) {
                match worker_server.recv_timeout(Duration::from_millis(200)) {
                    Ok(Some(request)) => handle(request,port,&origin,&shared,&worker_stop),
                    Ok(None)=>(), Err(e)=>{shared.logger.log(format!("HTTP receive failed: {e}"));break;}
                }
            }
            shared.logger.log("服务已停止，缓存文件保留。");
        });
        Ok(Self { stop, server, thread, port })
    }
    fn stop(&self) { self.stop.store(true,Ordering::SeqCst); self.server.unblock(); }
}
fn header<'a>(r: &'a Request, name: &'static str) -> Option<&'a str> { r.headers().iter().find(|h|h.field.equiv(name)).map(|h|h.value.as_str()) }
fn reply(request: Request, status: u16, body: Value, cors: Option<&str>, preflight: bool) {
    let private=header(&request,"Access-Control-Request-Private-Network")==Some("true");
    let mut response=Response::from_string(if preflight {String::new()}else{body.to_string()}).with_status_code(StatusCode(status));
    for (name,value) in [("Content-Type","application/json; charset=utf-8"),("Cache-Control","no-store"),("X-Content-Type-Options","nosniff"),("Referrer-Policy","no-referrer"),("Connection","close")] { response.add_header(Header::from_bytes(name,value).unwrap()); }
    if let Some(origin)=cors {
        response.add_header(Header::from_bytes("Access-Control-Allow-Origin",origin).unwrap());
        response.add_header(Header::from_bytes("Vary","Origin").unwrap());
        if preflight {
            response.add_header(Header::from_bytes("Access-Control-Allow-Methods","POST, OPTIONS").unwrap());
            response.add_header(Header::from_bytes("Access-Control-Allow-Headers","Authorization, Content-Type").unwrap());
            if private { response.add_header(Header::from_bytes("Access-Control-Allow-Private-Network","true").unwrap()); }
        }
    }
    let _=request.respond(response);
}
fn static_asset(path:&str)->Option<(&'static [u8],&'static str)> {
    match path {
        "/"|"/index.html"|"/indexv2.html"=>Some((include_bytes!(concat!(env!("OUT_DIR"),"/compressor.html")),"text/html; charset=utf-8")),
        "/res/jszip.min.js"=>Some((include_bytes!("../../res/jszip.min.js"),"text/javascript; charset=utf-8")),
        "/res/FileSaver.min.js"=>Some((include_bytes!("../../res/FileSaver.min.js"),"text/javascript; charset=utf-8")),
        "/res/jpeg-encode-worker.mjs"=>Some((include_bytes!("../../res/jpeg-encode-worker.mjs"),"text/javascript; charset=utf-8")),
        "/manifest.json"=>Some((include_bytes!("../../manifest.json"),"application/manifest+json")),
        _=>None
    }
}
fn serve_static(request:Request){
    let path=request.url().split('?').next().unwrap_or("");
    if let Some((bytes,kind))=static_asset(path){
        let mut response=Response::from_data(bytes).with_status_code(StatusCode(200));
        for (name,value) in [("Content-Type",kind),("Cache-Control","no-store"),("X-Content-Type-Options","nosniff"),("Referrer-Policy","no-referrer"),("Content-Security-Policy","default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' blob: data:; worker-src 'self' blob:; connect-src 'self' http://127.0.0.1:48731 http://127.0.0.1:48732 http://127.0.0.1:48733 http://127.0.0.1:48734 http://127.0.0.1:48735; object-src 'none'; base-uri 'none'; frame-ancestors 'none'")] {response.add_header(Header::from_bytes(name,value).unwrap());}
        let _=request.respond(response);
    }else{reply(request,404,json!({"error":"Not found"}),None,false);}
}
fn handle(mut request: Request, port: u16, origin: &str, shared: &Shared, stop: &AtomicBool) {
    let local=format!("http://127.0.0.1:{port}");
    let provided=header(&request,"Origin").unwrap_or("").to_owned();
    let allowed=!provided.is_empty() && (provided==origin || provided==local);
    let cors=if allowed {Some(provided.as_str())}else{None};
    if header(&request,"Host")!=Some(format!("127.0.0.1:{port}").as_str()) { reply(request,403,json!({"error":"Invalid host"}),cors,false); return; }
    if request.method()==&Method::Get {
        let site=header(&request,"Sec-Fetch-Site").unwrap_or("");
        if site=="cross-site" || (!provided.is_empty() && provided!=local){reply(request,403,json!({"error":"Cross-site resource request denied"}),None,false);}else{serve_static(request);}
        return;
    }
    if !allowed { reply(request,403,json!({"error":"Origin denied; save this compressor URL in the client"}),None,false); return; }
    if request.method()==&Method::Options { reply(request,204,json!(null),cors,true); return; }
    if request.method()!=&Method::Post { reply(request,404,json!({"error":"Not found"}),cors,false); return; }
    if header(&request,"Transfer-Encoding").is_some() || header(&request,"Content-Length").and_then(|v|v.parse::<u64>().ok()).is_none() { reply(request,411,json!({"error":"Content-Length required"}),cors,false); return; }
    let size=header(&request,"Content-Length").unwrap().parse::<u64>().unwrap();
    let endpoint=request.url().to_owned();
    let content_type=header(&request,"Content-Type").unwrap_or("").to_owned();
    let archive=endpoint=="/api/archives";
    if archive || endpoint=="/api/files" {
        let valid=if archive {size>0&&size<=MAX_ZIP}else{(4..20_000_000).contains(&size)};
        if !valid { reply(request,413,json!({"error":if archive {"ZIP transport limit is 250MB"}else{"JPEG must be smaller than 20,000,000 bytes"}}),cors,false); return; }
        if (archive&&content_type!="application/zip") || (!archive&&content_type.split(';').next()!=Some("image/jpeg")) { reply(request,415,json!({"error":"Unexpected Content-Type"}),cors,false); return; }
        let _guard=shared.cache_lock.lock().unwrap();
        match upload(&mut request,size,archive,shared,stop) {
            Ok(value)=> { shared.logger.log(if archive {"ZIP 已保存（原始文件，未重新打包）。"}else{"JPEG 已保存（未重新编码）。"}); reply(request,201,value,cors,false); }
            Err((status,error))=> { shared.logger.log(format!("Upload rejected: {error}")); reply(request,status,json!({"error":error}),cors,false); }
        }
        return;
    }
    if size>4096 { reply(request,413,json!({"error":"Request too large"}),cors,false); return; }
    let mut bytes=Vec::with_capacity(size as usize);
    if let Err(e)=request.as_reader().take(size).read_to_end(&mut bytes) { reply(request,400,json!({"error":e.to_string()}),cors,false); return; }
    if bytes.len() as u64 != size { reply(request,400,json!({"error":"Incomplete request"}),cors,false); return; }
    let body: Value=match serde_json::from_slice(&bytes) {Ok(v)=>v,Err(_)=>{reply(request,400,json!({"error":"Invalid JSON"}),cors,false);return;}};
    if stop.load(Ordering::SeqCst) { reply(request,503,json!({"error":"Service stopping"}),cors,false); return; }
    match endpoint.as_str() {
        "/api/health" => {shared.last_health.store(now_seconds(),Ordering::Relaxed);reply(request,200,json!({"service":"local-jpeg-clipboard","version":4,"archives":true,"clipboard":"file-url","codec":false}),cors,false)},
        "/api/clipboard/file" => {
            let path=body.get("fileId").and_then(Value::as_str).and_then(|id|shared.files.lock().unwrap().get(id).cloned());
            match path.filter(|p|p.is_file()) {
                None=>reply(request,404,json!({"error":"Unknown file ID"}),cors,false),
                Some(path)=>match shared.clipboard.copy(path) {
                    Ok(())=>{shared.logger.log("文件已复制，剪贴板回读验证通过。");reply(request,200,json!({"verified":true,"written":true,"format":"CF_HDROP","files":1}),cors,false);},
                    Err(e)=>{shared.logger.log(format!("Clipboard failed: {e}"));reply(request,400,json!({"error":e}),cors,false);}
                }
            }
        }
        _=>reply(request,404,json!({"error":"Not found"}),cors,false)
    }
}
fn upload(request: &mut Request, size:u64, archive:bool, shared:&Shared, stop:&AtomicBool) -> Result<Value,(u16,String)> {
    let run=|| -> Result<Value,String> {
        if cache_size(&shared.cache)?.checked_add(size).ok_or("Cache size overflow")?>MAX_CACHE { return Err("Cache limit reached (1GB); clear cache after pasting".into()); }
        let id=key()?;
        let tmp=shared.cache.join(format!("{id}.part"));
        let result=(|| -> Result<Value,String> {
            let mut output=io(OpenOptions::new().write(true).create_new(true).open(&tmp))?;
            let mut remaining=size;let mut buf=vec![0u8;1024*1024];let mut digest=Sha256::new();let mut first: Vec<u8>=Vec::new();let mut tail: Vec<u8>=Vec::new();
            while remaining>0 {
                if stop.load(Ordering::SeqCst) { return Err("Service stopping".into()); }
                let limit=(remaining as usize).min(buf.len());
                let n=io(request.as_reader().read(&mut buf[..limit]))?;
                if n==0 { return Err("Incomplete upload".into()); }
                if first.len()<3 { first.extend(buf[..n].iter().take(3-first.len())); }
                tail.extend_from_slice(&buf[..n]); if tail.len()>2 { tail.drain(..tail.len()-2); }
                digest.update(&buf[..n]);io(output.write_all(&buf[..n]))?;remaining-=n as u64;
            }
            io(output.flush())?;drop(output);
            if !archive && (first!=[255,216,255] || tail!=[255,217]) { return Err("JPEG signature invalid (not decoded by client)".into()); }
            let name=if archive {safe_zip_name(&tmp)?}else{format!("{id}.jpg")};
            let dir=if archive {let dir=shared.cache.join(&id);io(fs::create_dir(&dir))?;dir}else{shared.cache.clone()};
            let dest=dir.join(&name);
            if stop.load(Ordering::SeqCst) { return Err("Service stopping".into()); }
            io(fs::rename(&tmp,&dest))?;
            shared.files.lock().unwrap().insert(id.clone(),dest);
            Ok(json!({"fileId":id,"name":name,"bytes":size,"sha256":format!("{:x}",digest.finalize())}))
        })();
        // Only this randomly generated path is removed; no request supplies filesystem paths.
        if tmp.exists() { let _=fs::remove_file(&tmp); }
        result
    };
    let mut run=run;
    run().map_err(|e|(if e.starts_with("Cache limit") {507}else{400},e))
}

struct App {
    hwnd: HWND, url_edit: HWND, status: HWND, connection: HWND, clear_button: HWND, toggle_button: HWND, log_edit: HWND,
    last_cache_refresh: u64, built_in:bool, local_radio:HWND, custom_radio:HWND,
    root: PathBuf, saved_url: String, shared: Arc<Shared>, service: Option<Service>,
    pending_restart: bool, pending_clear: bool, log: String,
}
impl App {
    unsafe fn start(&mut self) {
        if self.service.is_some() { return; }
        match (if self.built_in{Ok(String::new())}else{validated_url(&self.saved_url).map(|(_,origin)|origin)}).and_then(|origin|Service::start(origin,self.shared.clone())) {
            Ok(service)=>{SetWindowTextW(self.status,wide(format!("● 服务运行中 · http://127.0.0.1:{}",service.port)).as_ptr());self.service=Some(service);},
            Err(e)=>{self.shared.logger.log(format!("Start failed: {e}"));SetWindowTextW(self.status,wide("○ 启动失败，请查看日志").as_ptr());}
        }
    }
    unsafe fn stop(&mut self) { if let Some(s)=&self.service {s.stop();SetWindowTextW(self.status,wide("○ 正在停止，等待当前传输完成…").as_ptr());} }
    unsafe fn tick(&mut self) {
        let now=now_seconds();let last=self.shared.last_health.load(Ordering::Relaxed);
        let connected=self.service.is_some() && last>0 && now.saturating_sub(last)<35;
        InvalidateRect(self.status,null(),1);
        let label=if connected {format!("● 网页已连接 · {}",if self.built_in{"内置压缩器"}else{self.saved_url.trim_end_matches('/')})}else{"○ 等待网页连接".to_owned()};
        SetWindowTextW(self.connection,wide(label).as_ptr());
        SetWindowTextW(self.toggle_button,wide(if self.service.is_some(){"停止服务"}else{"启动服务"}).as_ptr());
        if now.saturating_sub(self.last_cache_refresh)>=3 {
            if let Ok(bytes)=cache_size(&self.shared.cache) {SetWindowTextW(self.clear_button,wide(format!("清除 {:.1}MB 缓存",bytes as f64/1_000_000.0)).as_ptr());}
            self.last_cache_refresh=now;
        }
        if self.service.as_ref().is_some_and(|s|s.thread.is_finished()) {
            let service=self.service.take().unwrap();let _=service.thread.join();drop(service.server);
            SetWindowTextW(self.status,wide("○ 服务已停止").as_ptr());
        }
        if self.service.is_none() {
            if self.pending_clear {self.pending_clear=false;self.clear_cache();}
            if self.pending_restart {self.pending_restart=false;self.start();}
        }
    }
    unsafe fn clear_cache(&mut self) {
        // Service is fully stopped first. If clipboard cannot be queried, fail closed.
        if let Err(e)=verify_cache_directory(&self.shared.cache) {self.shared.logger.log(e);return;}
        let protected=match self.shared.clipboard.paths() {Ok(v)=>v,Err(e)=>{self.shared.logger.log(format!("Cleanup refused: {e}"));return;}};
        let _guard=self.shared.cache_lock.lock().unwrap();
        let mut removed=0; let mut failures=0;
        fn walk(root:&Path,protected:&[PathBuf],removed:&mut usize,failures:&mut usize) {
            let entries=match fs::read_dir(root){Ok(v)=>v,Err(_)=>{*failures+=1;return;}};
            for entry in entries.flatten() {
                let kind=match entry.file_type(){Ok(v)=>v,Err(_)=>{*failures+=1;continue;}};
                if fs::symlink_metadata(entry.path()).map_or(true,|m|m.file_attributes() & 0x400 != 0){continue;}
                let path=entry.path();
                if kind.is_dir(){walk(&path,protected,removed,failures);let _=fs::remove_dir(&path);}
                else if kind.is_file() && matches!(path.extension().and_then(|v|v.to_str()),Some("jpg"|"zip"|"part")) && !protected.iter().any(|p|p.to_string_lossy().eq_ignore_ascii_case(&path.to_string_lossy())) {
                    // path is an enumerated regular cache child, never an API-provided value.
                    match fs::remove_file(&path){Ok(())=>*removed+=1,Err(_)=>*failures+=1}
                }
            }
        }
        walk(&self.shared.cache,&protected,&mut removed,&mut failures);
        self.shared.files.lock().unwrap().retain(|_,p|p.is_file());
        self.shared.logger.log(format!("Cache cleared: {removed} files deleted, {failures} failures; clipboard-referenced files preserved."));
    }
    unsafe fn command(&mut self,id:usize) {
        match id {
            LOCAL_MODE|CUSTOM_MODE=>{
                self.built_in=id==LOCAL_MODE;
                InvalidateRect(self.local_radio,null(),1);InvalidateRect(self.custom_radio,null(),1);
                SendMessageW(self.local_radio,0x00F1,if self.built_in{1}else{0},0);SendMessageW(self.custom_radio,0x00F1,if self.built_in{0}else{1},0);
                EnableWindow(self.url_edit,if self.built_in{0}else{1});
                if let Err(e)=fs::write(self.root.join("compressor-mode.txt"),if self.built_in{"local"}else{"custom"}){alert(self.hwnd,&e.to_string());}
                self.pending_restart=true;self.stop();self.tick();
            }
            SAVE=>{
                if self.built_in{self.pending_restart=true;self.stop();self.tick();return;}

                let text=window_text(self.url_edit);
                match validated_url(&text) {
                    Err(e)=>alert(self.hwnd,&e),
                    Ok((url,_))=>{
                        match fs::write(self.root.join("compressor-url.txt"),&url) {
                            Err(e)=>alert(self.hwnd,&format!("Cannot save URL: {e}")),
                            Ok(())=>{self.saved_url=url;SetWindowTextW(self.url_edit,wide(&self.saved_url).as_ptr());self.pending_restart=true;self.stop();self.tick();}
                        }
                    }
                }
            }
            OPEN=>{
                if self.built_in{if let Some(service)=&self.service{shell_open(self.hwnd,&format!("http://127.0.0.1:{}/",service.port));}else{alert(self.hwnd,"请先启动服务");}return;}

                if window_text(self.url_edit).trim()!=self.saved_url.trim() {alert(self.hwnd,"URL changed. Click Save + restart first.");}
                else if self.service.is_none() {alert(self.hwnd,"Start the service first.");}
                else {shell_open(self.hwnd,&self.saved_url);}
            }
            START=>{self.pending_restart=true;self.tick();}
            STOP=>{if self.service.is_some(){self.pending_restart=false;self.stop();}else{self.start();}}
            CLEAR_ORIGIN=>{if MessageBoxW(self.hwnd,wide("恢复默认压缩器地址并重启服务？").as_ptr(),wide("重置允许来源").as_ptr(),MB_YESNO|MB_DEFBUTTON2)==IDYES{SetWindowTextW(self.url_edit,wide(DEFAULT_URL).as_ptr());self.built_in=false;SendMessageW(self.local_radio,0x00F1,0,0);SendMessageW(self.custom_radio,0x00F1,1,0);EnableWindow(self.url_edit,1);let _=fs::write(self.root.join("compressor-mode.txt"),"custom");self.command(SAVE);}}
            CACHE=>shell_open(self.hwnd,&self.shared.cache.to_string_lossy()),
            CLEAR=>{
                let answer=MessageBoxW(self.hwnd,wide("清除旧的 JPEG、ZIP 和临时缓存？\n\n当前剪贴板引用的文件会保留。旧网页复制按钮可能失效，清理时会暂停服务。").as_ptr(),wide("确认清除缓存").as_ptr(),MB_YESNO|MB_ICONWARNING|MB_DEFBUTTON2);
                if answer==IDYES {self.pending_clear=true;self.pending_restart=self.service.is_some();self.stop();self.tick();}
            }
            _=>()
        }
    }
    unsafe fn flush_log(&mut self) {
        let lines:Vec<String>=self.shared.logger.queue.lock().unwrap().drain(..).collect();
        for line in lines {self.log.push_str(&line);self.log.push_str("\r\n");}
        if self.log.len()>60000 {let mut start=self.log.len()-45000;while !self.log.is_char_boundary(start){start+=1;}self.log=self.log[start..].to_owned();}
        let recent=self.log.lines().filter(|line|!line.trim().is_empty()).rev().take(3)
            .map(|line|{let mut text=line.chars().take(92).collect::<String>();if line.chars().count()>92{text.push('…');}text})
            .collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\r\n");
        SetWindowTextW(self.log_edit,wide(&recent).as_ptr());
        SendMessageW(self.log_edit,0x00B1,usize::MAX,-1); // EM_SETSEL, keep log selectable
        SendMessageW(self.log_edit,0x00B7,0,0); // EM_SCROLLCARET
    }
}
unsafe fn window_text(hwnd:HWND)->String {let len=GetWindowTextLengthW(hwnd);let mut buf=vec![0u16;len as usize+1];let n=GetWindowTextW(hwnd,buf.as_mut_ptr(),buf.len() as i32);String::from_utf16_lossy(&buf[..n as usize])}
unsafe fn alert(hwnd:HWND,text:&str){MessageBoxW(hwnd,wide(text).as_ptr(),wide("JPEG Clipboard").as_ptr(),MB_OK|MB_ICONINFORMATION);}
unsafe fn shell_open(hwnd:HWND,target:&str){let result=ShellExecuteW(hwnd,wide("open").as_ptr(),wide(target).as_ptr(),null(),null(),SW_SHOWNORMAL);if result as isize<=32{alert(hwnd,"Windows could not open this URL / folder.");}}
unsafe extern "system" fn wndproc(hwnd:HWND,message:u32,wparam:WPARAM,lparam:LPARAM)->LRESULT {
    if message==WM_PAINT {
        let mut paint:PAINTSTRUCT=zeroed();let dc=BeginPaint(hwnd,&mut paint);FillRect(dc,&paint.rcPaint,BACKGROUND);
        let pen=CreatePen(PS_SOLID,1,if DARK{0x003e3636}else{0x00e5e5e5});let previous=SelectObject(dc,pen as _);let brush=SelectObject(dc,GetStockObject(NULL_BRUSH));
        MoveToEx(dc,px(28),px(126),null_mut());LineTo(dc,px(708),px(126));
        RoundRect(dc,px(27),px(198),px(709),px(226),px(7),px(7));SelectObject(dc,brush);SelectObject(dc,previous);DeleteObject(pen as _);EndPaint(hwnd,&paint);return 0;
    }
    if message==WM_SETTINGCHANGE || message==WM_THEMECHANGED {apply_theme(hwnd);return 0;}
    if message==WM_DRAWITEM {
        let item=&*(lparam as *const DRAWITEMSTRUCT);
        if item.CtlType==ODT_BUTTON {
            let dc=item.hDC;let mut rect=item.rcItem;
            FillRect(dc,&rect,BACKGROUND);
            let old_font=SelectObject(dc,UI_FONT as _);
            if item.CtlID==LOCAL_MODE as u32 || item.CtlID==CUSTOM_MODE as u32 {
                let app=GetWindowLongPtrW(hwnd,GWLP_USERDATA) as *const App;
                let checked=if app.is_null(){false}else{(*app).built_in==(item.CtlID==LOCAL_MODE as u32)};
                let x=rect.left+px(1);let y=rect.top+(rect.bottom-rect.top-px(14))/2;
                let color=if checked{0x00ff8a0a}else if DARK{0x00736a6a}else{0x00b6b6b6};
                let pen=CreatePen(PS_SOLID,px(1).max(1),color);let fill=CreateSolidBrush(if checked{color}else{if DARK{0x002c2424}else{0x00ffffff}});
                let prev_pen=SelectObject(dc,pen as _);let prev_fill=SelectObject(dc,fill as _);
                Ellipse(dc,x,y,x+px(14),y+px(14));
                if checked{let dot=CreateSolidBrush(0x00ffffff);let before=SelectObject(dc,dot as _);let before_pen=SelectObject(dc,GetStockObject(NULL_PEN));Ellipse(dc,x+px(5),y+px(5),x+px(10),y+px(10));SelectObject(dc,before_pen);SelectObject(dc,before);DeleteObject(dot as _);}
                SelectObject(dc,prev_pen);SelectObject(dc,prev_fill);DeleteObject(pen as _);DeleteObject(fill as _);
                rect.left+=px(22);SetBkMode(dc,TRANSPARENT as i32);SetTextColor(dc,if DARK{0x00dddddd}else{0x00333333});
                let label=wide(window_text(item.hwndItem));DrawTextW(dc,label.as_ptr(),-1,&mut rect,DT_LEFT|DT_VCENTER|DT_SINGLELINE);
                if item.itemState & ODS_FOCUS != 0{DrawFocusRect(dc,&rect);}
                SelectObject(dc,old_font);return 1;
            }
            SelectObject(dc,old_font);
            let pressed=item.itemState & ODS_SELECTED != 0;
            let fill=CreateSolidBrush(if DARK {if pressed{0x00483e3e}else{0x003d3333}}else{if pressed{0x00dedede}else{0x00f4f4f4}});
            let pen=CreatePen(PS_SOLID,1,if DARK{0x003d3333}else{0x00eeeeee});let old_pen=SelectObject(dc,pen as _);let old_brush=SelectObject(dc,fill as _);
            rect.left+=px(1);rect.top+=px(1);rect.right-=px(1);rect.bottom-=px(1);
            RoundRect(dc,rect.left,rect.top,rect.right,rect.bottom,px(7),px(7));SelectObject(dc,old_pen);SelectObject(dc,old_brush);DeleteObject(pen as _);DeleteObject(fill as _);
            let old_font=SelectObject(dc,UI_FONT as _);SetBkMode(dc,TRANSPARENT as i32);SetTextColor(dc,if DARK{0x00eeeeee}else{0x00333333});
            let label=wide(window_text(item.hwndItem));DrawTextW(dc,label.as_ptr(),-1,&mut rect,DT_CENTER|DT_VCENTER|DT_SINGLELINE);SelectObject(dc,old_font);
            if item.itemState & ODS_FOCUS != 0 {InflateRect(&mut rect,-px(3),-px(3));DrawFocusRect(dc,&rect);}
            return 1;
        }
    }
    if message==WM_CTLCOLORBTN {SetBkColor(wparam as HDC,if DARK{0x002c2424}else{0x00ffffff});return BACKGROUND as isize;}
    if message==WM_CTLCOLORSTATIC {
        let dc=wparam as HDC;SetBkMode(dc,OPAQUE as i32);SetBkColor(dc,if DARK{0x002c2424}else{0x00ffffff});let ptr=GetWindowLongPtrW(hwnd,GWLP_USERDATA) as *mut App;
        let state_label=!ptr.is_null() && (lparam as HWND==(*ptr).status || lparam as HWND==(*ptr).connection);
        let healthy=if ptr.is_null(){false}else if lparam as HWND==(*ptr).connection{let last=(&(*ptr).shared).last_health.load(Ordering::Relaxed);(*ptr).service.is_some()&&last>0&&now_seconds().saturating_sub(last)<35}else{(*ptr).service.is_some()};
        SetTextColor(dc,if state_label && healthy {0x0050b82b}else if state_label{0x00888888}else{if DARK{0x00dddddd}else{0x00444444}});return BACKGROUND as isize;
    }
    if message==WM_CTLCOLOREDIT {SetTextColor(wparam as HDC,if DARK{0x00eeeeee}else{0x00352b22});SetBkColor(wparam as HDC,if DARK{0x002c2424}else{0x00ffffff});return BACKGROUND as isize;}
    let ptr=GetWindowLongPtrW(hwnd,GWLP_USERDATA) as *mut App;
    if !ptr.is_null() {
        let app=&mut *ptr;
        match message {
            WM_COMMAND=>{app.command(wparam&0xffff);return 0;}
            WM_TIMER=>{app.tick();return 0;}
            WM_LOG=>{app.flush_log();return 0;}
            WM_CLOSE=>{
                app.pending_restart=false;app.stop();
                // Retain files for pasting. OS terminates remaining background threads on process exit.
                DestroyWindow(hwnd);return 0;
            }
            WM_DESTROY=>{PostQuitMessage(0);return 0;}
            _=>()
        }
    }
    DefWindowProcW(hwnd,message,wparam,lparam)
}
static mut UI_SCALE: f64 = 1.0;
static mut UI_FONT: HFONT = null_mut();
static mut TITLE_FONT: HFONT = null_mut();
static mut LOG_FONT: HFONT = null_mut();
static mut BACKGROUND: HBRUSH = null_mut();
static mut DARK: bool = false;
unsafe extern "system" fn repaint_child(hwnd:HWND,_:LPARAM)->BOOL {InvalidateRect(hwnd,null(),1);1}
unsafe fn apply_theme(hwnd:HWND) {
    let mut light:u32=1;let mut bytes=4u32;
    RegGetValueW(HKEY_CURRENT_USER,wide("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize").as_ptr(),wide("AppsUseLightTheme").as_ptr(),RRF_RT_REG_DWORD,null_mut(),&mut light as *mut _ as _,&mut bytes);
    DARK=light==0;
    let brush=CreateSolidBrush(if DARK{0x002c2424}else{0x00ffffff});
    let old=BACKGROUND;BACKGROUND=brush;
    if !hwnd.is_null() {
        SetClassLongPtrW(hwnd,GCLP_HBRBACKGROUND,brush as isize);
        let dark:i32=if DARK{1}else{0};DwmSetWindowAttribute(hwnd,20,&dark as *const _ as _,4);
        InvalidateRect(hwnd,null(),1);EnumChildWindows(hwnd,Some(repaint_child),0);
    }
    if !old.is_null(){DeleteObject(old as _);}
}
fn px(value:i32)->i32 { unsafe { (value as f64 * UI_SCALE).round() as i32 } }
unsafe fn make_font(size:i32,weight:i32,face:&str)->HFONT {
    CreateFontW(-px(size),0,0,0,weight,0,0,0,DEFAULT_CHARSET as u32,OUT_DEFAULT_PRECIS as u32,CLIP_DEFAULT_PRECIS as u32,CLEARTYPE_QUALITY as u32,DEFAULT_PITCH as u32,wide(face).as_ptr())
}
unsafe fn visual_styles(root:&Path) {
    let manifest=root.join("native-ui.manifest");
    let xml=r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0"><assemblyIdentity version="1.0.0.0" processorArchitecture="amd64" name="Nchyn.Clipboard.UI" type="win32"/><dependency><dependentAssembly><assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/></dependentAssembly></dependency></assembly>"#;
    if fs::write(&manifest,xml).is_ok() {
        let path=wide(manifest.as_os_str()); let mut context:ACTCTXW=zeroed();context.cbSize=size_of::<ACTCTXW>() as u32;context.lpSource=path.as_ptr();
        let handle=CreateActCtxW(&context);if handle!=INVALID_HANDLE_VALUE {let mut cookie=0;ActivateActCtx(handle,&mut cookie);}
    }
}
unsafe fn control(parent:HWND,class:&str,text:&str,style:u32,x:i32,y:i32,width:i32,height:i32,id:usize)->HWND {
    let hwnd=CreateWindowExW(0,wide(class).as_ptr(),wide(text).as_ptr(),WS_CHILD|WS_VISIBLE|style,px(x),px(y),px(width),px(height),parent,id as HMENU,GetModuleHandleW(null()),null());
    SendMessageW(hwnd,WM_SETFONT,UI_FONT as usize,1);
    hwnd
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn origins_are_canonical_and_restricted() {
        assert_eq!(validated_url("https://EXAMPLE.com:443/path?q=1").unwrap().1,"https://example.com");
        assert_eq!(validated_url("http://127.0.0.1:8080/path").unwrap().1,"http://127.0.0.1:8080");
        assert!(validated_url("http://example.com").is_err());
        assert!(validated_url("https://user:pass@example.com").is_err());
        assert!(validated_url("file:///C:/index.html").is_err());
        assert!(validated_url("javascript:alert(1)").is_err());
    }
    fn zip_fixture(names:&[&str]) -> PathBuf {
        let root=std::env::temp_dir().join(format!("jpeg-clipboard-test-{}",key().unwrap()));
        fs::create_dir(&root).unwrap();
        let path=root.join("fixture.zip");
        let mut zip=zip::ZipWriter::new(File::create(&path).unwrap());
        for name in names {
            zip.start_file(*name,zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored)).unwrap();
            zip.write_all(b"original bytes").unwrap();
        }
        zip.finish().unwrap();path
    }
    fn cleanup(path:&Path) { assert!(path.file_name()==Some(OsStr::new("fixture.zip")));fs::remove_file(path).unwrap();fs::remove_dir(path.parent().unwrap()).unwrap(); }
    #[test]
    fn zip_names_preserve_single_root_and_bytes() {
        let path=zip_fixture(&["照片/a.jpg","照片/b.txt"]);
        let before=fs::read(&path).unwrap();
        assert_eq!(safe_zip_name(&path).unwrap(),"照片.zip");
        assert_eq!(before,fs::read(&path).unwrap());cleanup(&path);
        let path=zip_fixture(&["a/x","b/x"]);
        assert_eq!(safe_zip_name(&path).unwrap(),"文件夹.zip");cleanup(&path);
        let path=zip_fixture(&["CON/file"]);
        assert_eq!(safe_zip_name(&path).unwrap(),"文件夹.zip");cleanup(&path);
    }
    #[test]
    fn zip_unsafe_paths_are_rejected() {
        for name in ["../bad","/absolute","root/../escape","C:/drive","root\\bad"] {
            let path=zip_fixture(&[name]);assert!(safe_zip_name(&path).is_err(),"{name}");cleanup(&path);
        }
    }
}
fn main() {
    unsafe {
        SetProcessDPIAware();
        let dc=GetDC(null_mut());UI_SCALE=GetDeviceCaps(dc,LOGPIXELSX as i32) as f64/96.0;ReleaseDC(null_mut(),dc);
        UI_FONT=make_font(12,400,"Microsoft YaHei UI");TITLE_FONT=make_font(21,600,"Microsoft YaHei UI");LOG_FONT=make_font(12,400,"Microsoft YaHei UI");apply_theme(null_mut());
        let local=match std::env::var_os("LOCALAPPDATA") {Some(p)=>PathBuf::from(p),None=>{alert(null_mut(),"LOCALAPPDATA is unavailable");return;}};
        let root=local.join("JPEGClipboardWindows");let cache=root.join("Cache");
        if let Err(e)=fs::create_dir_all(&cache){alert(null_mut(),&format!("Cannot create cache: {e}"));return;}
        visual_styles(&root);
        let built_in=fs::read_to_string(root.join("compressor-mode.txt")).map_or(true,|s|s.trim()!="custom");
        let saved_url=fs::read_to_string(root.join("compressor-url.txt")).ok().and_then(|s|validated_url(&s).ok().map(|v|v.0)).unwrap_or_else(||DEFAULT_URL.to_owned());
        let instance=GetModuleHandleW(null());let class=wide("JPEGClipboardWindowsNative");
        let mut wc:WNDCLASSW=zeroed();wc.lpfnWndProc=Some(wndproc);wc.hInstance=instance;wc.lpszClassName=class.as_ptr();wc.hCursor=LoadCursorW(null_mut(),IDC_ARROW);wc.hbrBackground=BACKGROUND;
        if RegisterClassW(&wc)==0 {alert(null_mut(),"Window registration failed");return;}
        let hwnd=CreateWindowExW(0,class.as_ptr(),wide("JPEG 剪贴板 · Windows 预览版 5.4").as_ptr(),WS_OVERLAPPED|WS_CAPTION|WS_SYSMENU|WS_MINIMIZEBOX|WS_CLIPCHILDREN,CW_USEDEFAULT,CW_USEDEFAULT,px(756),px(495),null_mut(),null_mut(),instance,null());
        if hwnd.is_null(){alert(null_mut(),"Cannot create main window");return;}
        apply_theme(hwnd);
        let title=control(hwnd,"STATIC","本机文件剪贴板",0,28,24,680,32,0);
        SendMessageW(title,WM_SETFONT,TITLE_FONT as usize,1);
        let status=control(hwnd,"STATIC","● 正在启动服务…",0,28,68,680,22,0);
        let connection=control(hwnd,"STATIC","○ 等待网页连接",0,28,98,680,22,0);

        let local_radio=control(hwnd,"BUTTON","内置压缩器",WS_TABSTOP|BS_OWNERDRAW as u32,28,140,160,24,LOCAL_MODE);
        let custom_radio=control(hwnd,"BUTTON","自定义压缩器 URL",WS_TABSTOP|BS_OWNERDRAW as u32,28,169,300,24,CUSTOM_MODE);
        SendMessageW(local_radio,0x00F1,if built_in{1}else{0},0);SendMessageW(custom_radio,0x00F1,if built_in{0}else{1},0);
        let url_edit=control(hwnd,"EDIT",&saved_url,WS_TABSTOP|ES_AUTOHSCROLL as u32,34,202,668,21,URL_ID);
        EnableWindow(url_edit,if built_in{0}else{1});
        SendMessageW(url_edit,0x00D3,3,8 | (8 << 16));
        control(hwnd,"STATIC","保存后直接连接，无令牌或配对弹窗。仅允许此 URL 来源，请填写可信网站。",0,28,244,680,22,0);
        let mut x=28;let mut clear_button=null_mut();let mut toggle_button=null_mut();
        for (text,id,width) in [("保存并重启",SAVE,102),("打开压缩器",OPEN,102),("停止服务",STOP,80),("打开缓存",CACHE,80),("清除 0.0MB 缓存",CLEAR,136),("重置允许来源",CLEAR_ORIGIN,100)] {
            let button=control(hwnd,"BUTTON",text,WS_TABSTOP|BS_OWNERDRAW as u32,x,277,width,25,id);if id==CLEAR{clear_button=button;}if id==STOP{toggle_button=button;}x+=width+8;
        }
        let log_edit=control(hwnd,"EDIT","",WS_TABSTOP|ES_MULTILINE as u32|ES_AUTOVSCROLL as u32|ES_READONLY as u32,28,321,680,64,LOG_ID);
        SendMessageW(log_edit,WM_SETFONT,LOG_FONT as usize,1);
        control(hwnd,"STATIC","客户端不编解码、不上传云端。仅保存网页生成的文件并写入文件剪贴板。",0,28,401,680,22,0);
        control(hwnd,"STATIC","关闭窗口即退出服务，缓存文件保留。",0,28,428,680,22,0);
        let logger=Logger{queue:Arc::new(Mutex::new(vec![])),hwnd:hwnd as isize};
        let clipboard=match Clipboard::spawn(){Ok(v)=>v,Err(e)=>{alert(hwnd,&e);return;}};
        let shared=Arc::new(Shared{last_health:AtomicU64::new(0),cache,files:Mutex::new(HashMap::new()),cache_lock:Mutex::new(()),clipboard,logger});
        let mut app=Box::new(App{hwnd,url_edit,status,connection,clear_button,toggle_button,last_cache_refresh:0,built_in,local_radio,custom_radio,log_edit,root,saved_url,shared,service:None,pending_restart:false,pending_clear:false,log:String::new()});
        SetWindowLongPtrW(hwnd,GWLP_USERDATA,&mut *app as *mut App as isize);
        InvalidateRect(local_radio,null(),1);InvalidateRect(custom_radio,null(),1);
        SetTimer(hwnd,1,200,None);ShowWindow(hwnd,SW_SHOWNORMAL);UpdateWindow(hwnd);app.start();
        let mut msg:MSG=zeroed();
        loop {let result=GetMessageW(&mut msg,null_mut(),0,0);if result<=0{break;}if msg.message==WM_KEYDOWN && msg.wParam==b'A' as usize && GetKeyState(VK_CONTROL as i32)<0 && (msg.hwnd==app.url_edit || msg.hwnd==app.log_edit) {SendMessageW(msg.hwnd,0x00B1,0,-1);continue;}if IsDialogMessageW(hwnd,&msg)==0{TranslateMessage(&msg);DispatchMessageW(&msg);}}
        if let Some(service)=&app.service{service.stop();}
    }
}
