use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::{HashMap,HashSet}, ffi::{CStr,CString,OsStr}, fs::{self,File}, io::{self,BufRead,BufReader,Read,Write}, net::{TcpListener,TcpStream}, os::{fd::{AsRawFd,FromRawFd},unix::{ffi::OsStrExt,fs::{MetadataExt,PermissionsExt}}}, path::{Component,Path,PathBuf}, process::{Command,Stdio}, sync::{Arc,Mutex,atomic::{AtomicBool,AtomicUsize,AtomicU64,Ordering}}, thread, time::{Duration,Instant,SystemTime,UNIX_EPOCH}};
const MAX_CACHE:u64=1_000_000_000;
const MAX_ZIP:u64=250_000_000;
const MAX_WORKERS:usize=8;
const HEADER_LIMIT:usize=16384;
const SOCKET_TIMEOUT:Duration=Duration::from_secs(15);
const REQUEST_TIMEOUT:Duration=Duration::from_secs(60);
type Result<T> = std::result::Result<T,String>;
fn io<T>(v:io::Result<T>)->Result<T>{v.map_err(|e|e.to_string())}
fn emit(v:Value){let mut out=std::io::stdout().lock();let _=writeln!(out,"{v}");let _=out.flush();}
fn key()->Result<String>{let mut b=[0u8;16];getrandom::getrandom(&mut b).map_err(|e|e.to_string())?;Ok(b.iter().map(|x|format!("{x:02x}")).collect())}
fn origin(text:&str)->Result<String>{let u=url::Url::parse(text).map_err(|_|"Invalid URL")?;if !u.username().is_empty()||u.password().is_some()||u.host_str().is_none()||!(u.scheme()=="https"||(u.scheme()=="http"&&matches!(u.host_str(),Some("127.0.0.1"|"localhost")))){return Err("Use HTTPS or loopback HTTP, without credentials".into())}Ok(u.origin().ascii_serialization())}
fn cstr(s:&OsStr)->Result<CString>{CString::new(s.as_bytes()).map_err(|_|"NUL in path".into())}
fn open_at(dir:&File,name:&OsStr,flags:i32,mode:u32)->Result<File>{let name=cstr(name)?;let fd=unsafe{libc::openat(dir.as_raw_fd(),name.as_ptr(),flags|libc::O_NOFOLLOW|libc::O_CLOEXEC,mode)};if fd<0{return Err(io::Error::last_os_error().to_string())}Ok(unsafe{File::from_raw_fd(fd)})}
fn unlink_at(dir:&File,name:&OsStr,is_dir:bool)->Result<()>{let name=cstr(name)?;let result=unsafe{libc::unlinkat(dir.as_raw_fd(),name.as_ptr(),if is_dir{libc::AT_REMOVEDIR}else{0})};if result!=0{return Err(io::Error::last_os_error().to_string())}Ok(())}
fn entries(dir:&File)->Result<Vec<(std::ffi::OsString,libc::stat)>>{
 let fd=unsafe{libc::openat(dir.as_raw_fd(),c".".as_ptr(),libc::O_RDONLY|libc::O_DIRECTORY|libc::O_CLOEXEC)};
 if fd<0{return Err(io::Error::last_os_error().to_string())}
 let stream=unsafe{libc::fdopendir(fd)};if stream.is_null(){unsafe{libc::close(fd)};return Err(io::Error::last_os_error().to_string())}
 let mut result=Vec::new();let run=(||{loop{let ent=unsafe{libc::readdir(stream)};if ent.is_null(){break}let name=unsafe{CStr::from_ptr((*ent).d_name.as_ptr())};if name.to_bytes()==b"."||name.to_bytes()==b".."{continue}let mut stat=unsafe{std::mem::zeroed()};if unsafe{libc::fstatat(dir.as_raw_fd(),name.as_ptr(),&mut stat,libc::AT_SYMLINK_NOFOLLOW)}!=0{return Err(io::Error::last_os_error().to_string())}result.push((OsStr::from_bytes(name.to_bytes()).to_os_string(),stat));if result.len()>100_000{return Err("Too many cache entries".into())}}Ok(result)})();unsafe{libc::closedir(stream)};run
}
struct Cache{path:PathBuf,dir:File}
impl Cache{
 fn new(path:PathBuf)->Result<Self>{
  let path=if path.is_absolute(){path}else{io(std::env::current_dir())?.join(path)};
  if path.components().any(|p|matches!(p,Component::ParentDir)){return Err("Cache path must not contain ..".into())}
  // Walk every component without following symlinks; create only missing private directories.
  let mut dir=io(File::open("/"))?;
  for part in path.components(){if let Component::Normal(name)=part {let n=cstr(name)?;let rc=unsafe{libc::mkdirat(dir.as_raw_fd(),n.as_ptr(),0o700)};if rc!=0&&io::Error::last_os_error().kind()!=io::ErrorKind::AlreadyExists{return Err(io::Error::last_os_error().to_string())}dir=open_at(&dir,name,libc::O_RDONLY|libc::O_DIRECTORY,0)?;}}
  let m=io(dir.metadata())?;if m.uid()!=unsafe{libc::geteuid()}{return Err("Cache must be owned by current user".into())}io(dir.set_permissions(fs::Permissions::from_mode(0o700)))?;Ok(Self{path,dir})
 }
 fn verify(&self)->Result<()>{let mut dir=io(File::open("/"))?;for part in self.path.components(){if let Component::Normal(name)=part{dir=open_at(&dir,name,libc::O_RDONLY|libc::O_DIRECTORY,0)?;}}let a=io(dir.metadata())?;let b=io(self.dir.metadata())?;if (a.dev(),a.ino())!=(b.dev(),b.ino()){return Err("Cache directory replaced".into())}Ok(())}
 fn size(&self)->Result<u64>{self.verify()?;fn walk(dir:&File,depth:usize)->Result<u64>{if depth>32{return Err("Cache nesting limit".into())}let mut total=0u64;for (name,m) in entries(dir)?{let kind=m.st_mode & libc::S_IFMT;if kind==libc::S_IFREG{if m.st_nlink!=1{return Err("Hard-linked cache file refused".into())}total=total.checked_add(m.st_size.try_into().map_err(|_|"Invalid cache size")?).ok_or("Cache overflow")?;}else if kind==libc::S_IFDIR{total=total.checked_add(walk(&open_at(dir,&name,libc::O_RDONLY|libc::O_DIRECTORY,0)?,depth+1)?).ok_or("Cache overflow")?;}else{return Err("Symlink or special file in cache; operation refused".into())}}Ok(total)}walk(&self.dir,0)}
 fn regular(&self,relative:&Path)->Result<File>{self.verify()?;let mut dir=io(self.dir.try_clone())?;let parts:Vec<_>=relative.components().collect();for (i,part) in parts.iter().enumerate(){let Component::Normal(name)=part else{return Err("Invalid generated cache path".into())};dir=open_at(&dir,name,libc::O_RDONLY|if i+1<parts.len(){libc::O_DIRECTORY}else{0},0)?;}let m=io(dir.metadata())?;if !m.is_file()||m.nlink()!=1{return Err("Unsafe cached file".into())}Ok(dir)}
}
// zip 2.x deduplicates its index by filename. Validate raw central records first,
// so duplicate names, hidden expansion and entry-count bombs cannot disappear.
fn validate_central(file:&File)->Result<()>{
 use std::os::unix::fs::FileExt;
 let size=io(file.metadata())?.len();let len=size.min(65557) as usize;let mut tail=vec![0;len];io(file.read_exact_at(&mut tail,size-len as u64))?;
 let u16le=|b:&[u8],p:usize|u16::from_le_bytes([b[p],b[p+1]]) as u64;let u32le=|b:&[u8],p:usize|u32::from_le_bytes(b[p..p+4].try_into().unwrap()) as u64;
 let pos=(0..len.saturating_sub(21)).rev().find(|&p|tail[p..].starts_with(b"PK\x05\x06")&&p+22+u16le(&tail,p+20) as usize==len).ok_or("Missing ZIP end record")?;
 if u16le(&tail,pos+4)!=0||u16le(&tail,pos+6)!=0||u16le(&tail,pos+8)!=u16le(&tail,pos+10){return Err("Multi-disk ZIP refused".into())}
 let count=u16le(&tail,pos+10);let central_size=u32le(&tail,pos+12);let start=u32le(&tail,pos+16);
 if count==65535||start==u32::MAX as u64||central_size==u32::MAX as u64{return Err("ZIP64 transport metadata is unsupported".into())}if count>10000{return Err("ZIP entry count limit is 10,000".into())}
 let end=start.checked_add(central_size).ok_or("ZIP directory overflow")?;if end>size-len as u64+pos as u64{return Err("Invalid ZIP directory range".into())}
 let mut offset=start;let mut total=0u64;let mut names=HashSet::new();
 for _ in 0..count{if offset+46>end{return Err("Truncated ZIP directory".into())}let mut h=[0;46];io(file.read_exact_at(&mut h,offset))?;if &h[..4]!=b"PK\x01\x02"{return Err("Invalid ZIP directory entry".into())}let flags=u16le(&h,8);let expanded=u32le(&h,24);let n=u16le(&h,28);let extra=u16le(&h,30);let comment=u16le(&h,32);if flags&1!=0||flags&64!=0{return Err("Encrypted ZIP refused".into())}if expanded==u32::MAX as u64{return Err("ZIP64 entry unsupported".into())}total=total.checked_add(expanded).ok_or("ZIP expansion overflow")?;if total>MAX_ZIP{return Err("ZIP expanded size limit is 250MB".into())}let next=offset+46+n+extra+comment;if next>end{return Err("Invalid ZIP entry bounds".into())}let mut name=vec![0;n as usize];io(file.read_exact_at(&mut name,offset+46))?;if name.contains(&0)||!names.insert(name){return Err("Duplicate or NUL ZIP entry".into())}offset=next;
 }if offset!=end{return Err("Unexpected ZIP directory records".into())}Ok(())
}
fn safe_zip_name(file:&File)->Result<String>{
 validate_central(file)?;
 let mut archive=zip::ZipArchive::new(io(file.try_clone())?).map_err(|e|e.to_string())?;
 if archive.len()>10000{return Err("ZIP entry count limit is 10,000".into())}
 let mut expanded=0u64;let mut names=HashSet::new();let mut roots=HashSet::new();
 for i in 0..archive.len(){let e=archive.by_index_raw(i).map_err(|e|e.to_string())?;let n=e.name();expanded=expanded.checked_add(e.size()).ok_or("ZIP size overflow")?;if expanded>MAX_ZIP{return Err("ZIP expanded size limit is 250MB".into())}
  if e.encrypted()||n.is_empty()||n.starts_with('/')||n.contains('\\')||n.contains('\0')||n.as_bytes().get(1)==Some(&b':')||n.split('/').any(|p|p==".."||p==".")||!names.insert(n.trim_end_matches('/').to_owned())||matches!(e.unix_mode(),Some(mode) if mode&0o170000==0o120000){return Err("Unsafe ZIP entry".into())}
  roots.insert(n.split('/').next().unwrap().to_owned());
 }
 let root=if roots.len()==1{roots.into_iter().next().unwrap()}else{"文件夹".into()};let name:String=root.chars().map(|c|if c.is_control()||"<>:\"/\\|?*".contains(c){'_'}else{c}).take(100).collect();let name=name.trim_matches(['.',' ']);Ok(format!("{}.zip",if name.is_empty(){"文件夹"}else{name}))
}
struct Shared{cache:Cache,files:Mutex<HashMap<String,PathBuf>>,cache_lock:Mutex<()>,helper:PathBuf,last_health:AtomicU64,stop:AtomicBool,active:AtomicUsize,origin:Option<String>,port:u16}
struct Request{method:String,path:String,headers:HashMap<String,String>,reader:BufReader<TcpStream>,start:Instant}
impl Request{
 fn read(stream:TcpStream)->Result<Self>{io(stream.set_read_timeout(Some(SOCKET_TIMEOUT)))?;io(stream.set_write_timeout(Some(SOCKET_TIMEOUT)))?;let mut reader=BufReader::with_capacity(8192,stream);let start=Instant::now();let mut bytes=Vec::new();let mut one=[0];loop{if bytes.len()>=HEADER_LIMIT{return Err("HTTP headers too large".into())}if start.elapsed()>SOCKET_TIMEOUT{return Err("HTTP header deadline exceeded".into())}if io(reader.read(&mut one))?==0{return Err("Incomplete HTTP headers".into())}bytes.push(one[0]);if bytes.ends_with(b"\r\n\r\n"){break}}
 let text=std::str::from_utf8(&bytes).map_err(|_|"Invalid HTTP headers")?;let mut lines=text.split("\r\n");let first=lines.next().unwrap().split(' ').collect::<Vec<_>>();if first.len()!=3||!matches!(first[2],"HTTP/1.1"|"HTTP/1.0")||!first[1].starts_with('/')||first[1].contains('#'){return Err("Invalid HTTP request line".into())}
 let mut headers=HashMap::new();for line in lines{if line.is_empty(){break}let (name,value)=line.split_once(':').ok_or("Invalid header")?;if name.is_empty()||!name.bytes().all(|b|b.is_ascii_alphanumeric()||b"!#$%&'*+-.^_`|~".contains(&b))||value.bytes().any(|b|(b<32&&b!=b'\t')||b==127){return Err("Invalid header".into())}if headers.insert(name.to_ascii_lowercase(),value.trim().to_owned()).is_some(){return Err("Duplicate HTTP header refused".into())}}
 Ok(Self{method:first[0].into(),path:first[1].into(),headers,reader,start})
 }
 fn header(&self,name:&str)->&str{self.headers.get(name).map(String::as_str).unwrap_or("")}
 fn chunk(&mut self,buf:&mut[u8],stop:&AtomicBool)->Result<usize>{if stop.load(Ordering::SeqCst){return Err("Service stopping".into())}let left=REQUEST_TIMEOUT.checked_sub(self.start.elapsed()).ok_or("Request deadline exceeded")?;io(self.reader.get_ref().set_read_timeout(Some(left.min(SOCKET_TIMEOUT))))?;io(self.reader.read(buf))}
 fn reply(&mut self,status:u16,body:&[u8],kind:&str,cors:Option<&str>,preflight:bool,nonce:&str){
 let reason=match status{200=>"OK",201=>"Created",204=>"No Content",400=>"Bad Request",403=>"Forbidden",404=>"Not Found",408=>"Request Timeout",411=>"Length Required",413=>"Content Too Large",415=>"Unsupported Media Type",503=>"Service Unavailable",507=>"Insufficient Storage",_=>"Error"};
 let mut head=format!("HTTP/1.1 {status} {reason}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'self'; script-src 'self' 'nonce-{nonce}'; style-src 'self' 'unsafe-inline'; img-src 'self' blob: data:; worker-src 'self' blob:; connect-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'\r\n",body.len());
 if let Some(origin)=cors{head.push_str(&format!("Access-Control-Allow-Origin: {origin}\r\nVary: Origin\r\n"));if preflight{head.push_str("Access-Control-Allow-Methods: POST, OPTIONS\r\nAccess-Control-Allow-Headers: Authorization, Content-Type\r\n");if self.header("access-control-request-private-network")=="true"{head.push_str("Access-Control-Allow-Private-Network: true\r\n")}}}head.push_str("\r\n");let socket=self.reader.get_mut();let _=socket.write_all(head.as_bytes());let _=socket.write_all(body);
 }
 fn json(&mut self,status:u16,body:Value,cors:Option<&str>){self.reply(status,body.to_string().as_bytes(),"application/json; charset=utf-8",cors,false,"")}
}
fn static_asset(path:&str)->Option<(&'static[u8],&'static str)>{match path.split('?').next().unwrap_or(""){
 "/"|"/index.html"|"/indexv2.html"=>Some((include_bytes!(concat!(env!("OUT_DIR"),"/compressor.html")),"text/html; charset=utf-8")),
 "/res/jszip.min.js"=>Some((include_bytes!("../../res/jszip.min.js"),"text/javascript; charset=utf-8")),
 "/res/FileSaver.min.js"=>Some((include_bytes!("../../res/FileSaver.min.js"),"text/javascript; charset=utf-8")),
 "/res/jpeg-encode-worker.mjs"=>Some((include_bytes!("../../res/jpeg-encode-worker.mjs"),"text/javascript; charset=utf-8")),
 "/manifest.json"=>Some((include_bytes!("../../manifest.json"),"application/manifest+json")),_=>None}}
fn upload(r:&mut Request,size:u64,archive:bool,s:&Shared)->Result<Value>{
 let _lock=s.cache_lock.lock().unwrap();if s.cache.size()?.checked_add(size).ok_or("Cache overflow")?>MAX_CACHE{return Err("Cache limit reached (1GB)".into())}let id=key()?;let tmp=format!("{id}.part");let run=(||{
  let mut file=open_at(&s.cache.dir,OsStr::new(&tmp),libc::O_RDWR|libc::O_CREAT|libc::O_EXCL,0o600)?;let mut remaining=size;let mut buf=vec![0;1024*1024];let mut hash=Sha256::new();let mut first:Vec<u8>=Vec::new();let mut tail=Vec::new();while remaining>0{let limit=buf.len().min(remaining as usize);let n=r.chunk(&mut buf[..limit],&s.stop)?;if n==0{return Err("Incomplete upload".into())}first.extend(buf[..n].iter().take(3-first.len()));tail.extend_from_slice(&buf[..n]);if tail.len()>2{tail.drain(..tail.len()-2);}hash.update(&buf[..n]);io(file.write_all(&buf[..n]))?;remaining-=n as u64;}
  io(file.flush())?;if !archive&&(first!=[255,216,255]||tail!=[255,217]){return Err("JPEG signature invalid (not decoded by client)".into())}
  let name=if archive{safe_zip_name(&file)?}else{format!("{id}.jpg")};s.cache.verify()?;let target=if archive{let n=cstr(OsStr::new(&id))?;if unsafe{libc::mkdirat(s.cache.dir.as_raw_fd(),n.as_ptr(),0o700)}!=0{return Err(io::Error::last_os_error().to_string())}open_at(&s.cache.dir,OsStr::new(&id),libc::O_RDONLY|libc::O_DIRECTORY,0)?}else{io(s.cache.dir.try_clone())?};
  if s.stop.load(Ordering::SeqCst){return Err("Service stopping".into())}let from=cstr(OsStr::new(&tmp))?;let dest=cstr(OsStr::new(&name))?;if unsafe{libc::renameat(s.cache.dir.as_raw_fd(),from.as_ptr(),target.as_raw_fd(),dest.as_ptr())}!=0{return Err(io::Error::last_os_error().to_string())}let relative=if archive{PathBuf::from(&id).join(&name)}else{PathBuf::from(&name)};s.files.lock().unwrap().insert(id.clone(),relative);Ok(json!({"fileId":id,"name":name,"bytes":size,"sha256":format!("{:x}",hash.finalize())}))
 })();let _=unlink_at(&s.cache.dir,OsStr::new(&tmp),false);run
}
fn helper(s:&Shared,path:&Path)->Result<Value>{
 let mut child=io(Command::new(&s.helper).arg(path).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn())?;
 let mut output=child.stdout.take().unwrap();unsafe{libc::fcntl(output.as_raw_fd(),libc::F_SETFL,libc::O_NONBLOCK);}
 let mut bytes=Vec::new();let start=Instant::now();
 loop{let mut buf=[0u8;4096];loop{match output.read(&mut buf){Ok(0)=>break,Ok(n)=>{bytes.extend_from_slice(&buf[..n]);if bytes.len()>65536{let _=child.kill();let _=child.wait();return Err("Helper output too large".into())}},Err(e)if e.kind()==io::ErrorKind::WouldBlock=>break,Err(e)=>{let _=child.kill();let _=child.wait();return Err(e.to_string())}}}
 match child.try_wait(){Ok(Some(status))=>{ // Drain any final bytes already buffered in the pipe.
 loop{match output.read(&mut buf){Ok(0)=>break,Ok(n)=>bytes.extend_from_slice(&buf[..n]),Err(_)=>break}}
if !status.success(){return Err("Clipboard helper failed".into())}if bytes.len()>65536{return Err("Helper output too large".into())}let report:Value=serde_json::from_slice(&bytes).map_err(|_|"Invalid helper JSON")?;if report.get("verified")!=Some(&Value::Bool(true))||report.get("written")!=Some(&Value::Bool(true)){return Err("Clipboard helper did not verify file URL".into())}return Ok(report)},Ok(None)=>(),Err(e)=>{let _=child.kill();let _=child.wait();return Err(e.to_string())}}if start.elapsed()>Duration::from_secs(15)||s.stop.load(Ordering::SeqCst){let _=child.kill();let _=child.wait();return Err("Clipboard helper timeout or shutdown".into())}thread::sleep(Duration::from_millis(20));}
}
fn handle(mut r:Request,s:&Shared){
 let local=format!("http://127.0.0.1:{}",s.port);let provided=r.header("origin").to_owned();let allowed=!provided.is_empty()&&(provided==local||s.origin.as_deref()==Some(&provided));let cors=allowed.then_some(provided.as_str());
 if r.header("host")!=format!("127.0.0.1:{}",s.port){r.json(403,json!({"error":"Invalid host"}),None);return}
 if r.method=="GET"{if r.header("sec-fetch-site")=="cross-site"||(!provided.is_empty()&&provided!=local){r.json(403,json!({"error":"Cross-site resource request denied"}),None);return}if let Some((bytes,kind))=static_asset(&r.path){if kind.starts_with("text/html"){match key(){Ok(nonce)=>{let text=String::from_utf8_lossy(bytes).replace("<script",&format!("<script nonce=\"{nonce}\""));r.reply(200,text.as_bytes(),kind,None,false,&nonce)},Err(e)=>r.json(503,json!({"error":e}),None)}}else{r.reply(200,bytes,kind,None,false,"")}}else{r.json(404,json!({"error":"Not found"}),None)}return}
 if !allowed{r.json(403,json!({"error":"Origin denied"}),None);return}if r.method=="OPTIONS"{r.reply(204,b"","application/json",cors,true,"");return}if r.method!="POST"{r.json(404,json!({"error":"Not found"}),cors);return}
 let size=if r.headers.contains_key("transfer-encoding"){None}else{r.header("content-length").parse::<u64>().ok()};let Some(size)=size else{r.json(411,json!({"error":"Content-Length required"}),cors);return};let archive=r.path=="/api/archives";
 if archive||r.path=="/api/files"{if !(if archive{size>0&&size<=MAX_ZIP}else{(4..20_000_000).contains(&size)}){r.json(413,json!({"error":"Upload size limit exceeded"}),cors);return}if (archive&&r.header("content-type")!="application/zip")||(!archive&&r.header("content-type").split(';').next()!=Some("image/jpeg")){r.json(415,json!({"error":"Unexpected Content-Type"}),cors);return}match upload(&mut r,size,archive,s){Ok(body)=>r.json(201,body,cors),Err(e)=>r.json(if e.starts_with("Cache limit"){507}else{400},json!({"error":e}),cors)}return}
 if size>4096{r.json(413,json!({"error":"Request too large"}),cors);return}let mut bytes=vec![0;size as usize];let body:Result<Value>=(||{let mut offset=0;while offset<bytes.len(){let n=r.chunk(&mut bytes[offset..],&s.stop)?;if n==0{return Err("Incomplete JSON body".into())}offset+=n}serde_json::from_slice::<Value>(&bytes).map_err(|_|"Invalid JSON".into())})();let body=match body{Ok(v)=>v,Err(e)=>{r.json(400,json!({"error":e}),cors);return}};
 match r.path.as_str(){"/api/health"=>{let now=SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();let old=s.last_health.load(Ordering::Relaxed);if now.saturating_sub(old)>=5&&s.last_health.compare_exchange(old,now,Ordering::Relaxed,Ordering::Relaxed).is_ok(){emit(json!({"event":"connected","origin":provided}))}r.json(200,json!({"service":"local-jpeg-clipboard","version":4,"archives":true,"clipboard":"file-url","codec":false}),cors)},"/api/clipboard/file"=>{
 let path=body.get("fileId").and_then(Value::as_str).and_then(|id|s.files.lock().unwrap().get(id).cloned());let Some(relative)=path else{r.json(404,json!({"error":"Unknown file ID"}),cors);return};let _lock=s.cache_lock.lock().unwrap();let result:Result<Value>=(||{let file=s.cache.regular(&relative)?;let bytes=io(file.metadata())?.len();let report=helper(s,&s.cache.path.join(&relative))?;emit(json!({"event":"clipboard","name":relative.file_name().unwrap().to_string_lossy(),"bytes":bytes}));Ok(report)})();match result{Ok(report)=>r.json(200,report,cors),Err(e)=>r.json(400,json!({"error":e}),cors)}},_=>r.json(404,json!({"error":"Not found"}),cors)}
}
fn bind(port:u16)->Result<TcpListener>{let ports=if port==48731{(48731..=48735).collect::<Vec<_>>()}else{vec![port]};for p in ports{match TcpListener::bind(("127.0.0.1",p)){Ok(l)=>return Ok(l),Err(e)if e.kind()==io::ErrorKind::AddrInUse=>continue,Err(e)=>return Err(e.to_string())}}Err("Ports 48731–48735 unavailable; close duplicate clients".into())}
struct Args{port:u16,cache:PathBuf,helper:PathBuf,origin:Option<String>,events:bool,no_open:bool}
fn args()->Result<Args>{let exe=io(std::env::current_exe())?;let home=std::env::var_os("HOME").ok_or("HOME unavailable")?;let mut a=Args{port:48731,cache:PathBuf::from(home).join("Library/Caches/LocalJPEGClipboard"),helper:exe.parent().unwrap().join("clipboard-helper"),origin:None,events:false,no_open:true};let mut it=std::env::args().skip(1);while let Some(arg)=it.next(){match arg.as_str(){"--port"=>a.port=it.next().ok_or("Missing port")?.parse().map_err(|_|"Invalid port")?,"--cache-dir"=>a.cache=it.next().ok_or("Missing cache directory")?.into(),"--helper"=>a.helper=it.next().ok_or("Missing helper")?.into(),"--origin"=>a.origin=Some(origin(&it.next().ok_or("Missing origin")?)?),"--json-events"=>a.events=true,"--no-open"=>a.no_open=true,"--help"=>{eprintln!("jpeg-clipboard-service [--port 48731] [--no-open] [--json-events] [--cache-dir PATH] [--helper PATH] [--origin URL]");std::process::exit(0)},_=>return Err(format!("Unknown option: {arg}"))}}Ok(a)}
fn run()->Result<()>{let a=args()?;let cache=Cache::new(a.cache)?;if !a.helper.is_file(){return Err("Clipboard helper missing; provide --helper PATH".into())}let listener=bind(a.port)?;io(listener.set_nonblocking(true))?;let port=io(listener.local_addr())?.port();let s=Arc::new(Shared{cache,files:Mutex::new(HashMap::new()),cache_lock:Mutex::new(()),helper:a.helper,last_health:AtomicU64::new(0),stop:AtomicBool::new(false),active:AtomicUsize::new(0),origin:a.origin,port});
 if a.events{let shared=s.clone();thread::spawn(move||{let stdin=std::io::stdin();let mut reader=stdin.lock();loop{let mut line=Vec::new();match reader.by_ref().take(8193).read_until(b'\n',&mut line){Ok(0)=>break,Ok(_)if line.len()<=8192=>{if let Ok(v)=serde_json::from_slice::<Value>(&line){if v.get("command").or_else(||v.get("cmd")).and_then(Value::as_str)==Some("shutdown"){break}emit(json!({"event":"error","message":"Unsupported stdin command; cache cleanup is owned by the native UI"}));}},_=>break}}shared.stop.store(true,Ordering::SeqCst);});}
 emit(json!({"event":"ready","endpoint":format!("http://127.0.0.1:{port}")}));if !a.no_open{let _=Command::new("/usr/bin/open").arg(format!("http://127.0.0.1:{port}/indexv2.html")).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();}
 while !s.stop.load(Ordering::SeqCst){match listener.accept(){Ok((stream,_))=>{if s.active.fetch_add(1,Ordering::SeqCst)>=MAX_WORKERS{s.active.fetch_sub(1,Ordering::SeqCst);let _=stream.shutdown(std::net::Shutdown::Both);continue}let shared=s.clone();thread::spawn(move||{if let Ok(r)=Request::read(stream){handle(r,&shared)}shared.active.fetch_sub(1,Ordering::SeqCst);});},Err(e)if e.kind()==io::ErrorKind::WouldBlock=>thread::sleep(Duration::from_millis(20)),Err(e)=>return Err(e.to_string())}}
 // Process exit never waits for stalled workers. Completed clipboard files stay on disk.
 Ok(())}
fn main(){if let Err(e)=run(){emit(json!({"event":"error","message":e}));std::process::exit(1)}}
