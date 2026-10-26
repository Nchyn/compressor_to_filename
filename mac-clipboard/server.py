#!/usr/bin/env python3
"""Loopback file clipboard bridge. No image codec; Python stdlib + Swift helper."""
import argparse, base64, hashlib, json, os, re, secrets, shutil, subprocess, threading, time, webbrowser
from pathlib import Path
from urllib.parse import urlsplit
from http.server import ThreadingHTTPServer, BaseHTTPRequestHandler

ROOT = Path(__file__).resolve().parent.parent
CACHE = Path.home() / 'Library' / 'Caches' / 'LocalJPEGClipboard'
MAX_FILE = 20_000_000  # strict less than this
MAX_CACHE = 1_000_000_000
TOKEN = secrets.token_urlsafe(32)
LOCK = threading.Lock()
FILES = {}
EXTRA_ORIGIN = None
ALLOW_FILE = False
LAST_ACTIVITY = 0
def emit(event, **values):
    print(json.dumps({'event':event, **values}), flush=True)
def url_origin(value):
    u=urlsplit(value)
    if u.username or u.password or not u.hostname: raise ValueError('Invalid URL')
    if u.scheme != 'https' and not (u.scheme == 'http' and u.hostname in ('127.0.0.1','localhost')):
        raise ValueError('Use HTTPS, or HTTP on localhost')
    port=u.port
    return u.scheme+'://'+u.hostname+((':'+str(port)) if port and port != (443 if u.scheme=='https' else 80) else '')
RAIL = 'indexv2.html'
ALLOWED = {'/': RAIL, '/'+RAIL: RAIL, '/jpeg-clipboard.html':'jpeg-clipboard.html', '/res/jpeg-ui.mjs': 'res/jpeg-ui.mjs', '/res/jpeg-encode-worker.mjs': 'res/jpeg-encode-worker.mjs', '/res/jszip.min.js':'res/jszip.min.js', '/res/FileSaver.min.js':'res/FileSaver.min.js'}

class Handler(BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'
    def log_message(self, *_): pass  # no URLs, tokens, filenames, or picture data in logs
    def allowed_origin(self):
        origin=self.headers.get('Origin')
        return bool(origin) and (origin==f'http://127.0.0.1:{self.server.server_port}' or origin==EXTRA_ORIGIN or (ALLOW_FILE and origin=='null'))
    def valid_host(self):
        return self.headers.get('Host') == f'127.0.0.1:{self.server.server_port}'
    def respond(self, code, body, kind='application/json'):
        if not isinstance(body, bytes): body = json.dumps(body, ensure_ascii=False).encode()
        self.send_response(code)
        self.send_header('Content-Type', kind)
        if self.allowed_origin():
            self.send_header('Access-Control-Allow-Origin', self.headers['Origin'])
            self.send_header('Vary', 'Origin')
        self.send_header('Content-Length', str(len(body)))
        self.send_header('Cache-Control', 'no-store')
        self.send_header('X-Content-Type-Options', 'nosniff')
        self.send_header('Referrer-Policy', 'no-referrer')
        nonce = getattr(self, 'script_nonce', '')
        self.send_header('Content-Security-Policy', f"default-src 'self'; script-src 'self' 'nonce-{nonce}'; style-src 'self' 'unsafe-inline'; img-src 'self' blob: data:; connect-src 'self' https://cpr-api.nchyn.net; worker-src 'self' blob:; frame-ancestors 'none'; base-uri 'none'")
        self.end_headers()
        try: self.wfile.write(body)
        except (BrokenPipeError, ConnectionResetError): pass
    def authorized(self):
        expected = f'http://127.0.0.1:{self.server.server_port}'
        return self.valid_host() and self.allowed_origin() and secrets.compare_digest(self.headers.get('Authorization', ''), 'Bearer ' + TOKEN)
    def do_OPTIONS(self):
        if not self.valid_host() or not self.allowed_origin():
            return self.respond(403, {'error':'Origin denied'})
        self.send_response(204)
        self.send_header('Access-Control-Allow-Origin', self.headers['Origin'])
        self.send_header('Access-Control-Allow-Methods', 'POST, OPTIONS')
        self.send_header('Access-Control-Allow-Headers', 'Authorization, Content-Type')
        if self.headers.get('Access-Control-Request-Private-Network') == 'true':
            self.send_header('Access-Control-Allow-Private-Network', 'true')
        self.send_header('Content-Length', '0')
        self.end_headers()
    def do_GET(self):
        if not self.valid_host(): return self.respond(403, {'error':'Invalid host'})
        path = ALLOWED.get(self.path)
        if not path: return self.respond(404, {'error':'Not found'})
        p=ROOT/path
        content=p.read_bytes()
        if path.endswith('.html'):
            self.script_nonce=base64.b64encode(secrets.token_bytes(18)).decode()
            text=content.decode()
            if path == RAIL:
                # Remove third-party analytics from this authenticated clipboard page.
                text=re.sub(r'<script\b[^>]*>[\s\S]*?</script>', lambda m: '' if ('googletagmanager.com' in m[0] or 'hm.baidu.com' in m[0]) else m[0], text)
            text=re.sub(r'<script\b', '<script nonce="'+self.script_nonce+'"', text)
            content=text.encode()
        self.respond(200,content,'text/html; charset=utf-8' if path.endswith('.html') else 'text/javascript; charset=utf-8')
    def do_POST(self):
        self.close_connection=True
        if not self.authorized(): return self.respond(403, {'error':'Origin or session token invalid. Reopen the client page.'})
        try:
            size=int(self.headers.get('Content-Length','-1'))
            if self.headers.get('Transfer-Encoding') or size<0: return self.respond(411,{'error':'Content-Length required'})
            self.connection.settimeout(60)
            if self.path == '/api/files':
                if not 4 <= size < MAX_FILE: return self.respond(413,{'error':'JPEG must be smaller than 20,000,000 bytes'})
                if self.headers.get('Content-Type','').split(';')[0]!='image/jpeg': return self.respond(415,{'error':'Expected image/jpeg'})
                # Serializes uploads and cache accounting. No arbitrary client paths or file names.
                with LOCK:
                    used=sum(p.stat().st_size for p in CACHE.glob('*') if p.is_file())
                    if used+size>MAX_CACHE: return self.respond(507,{'error':'Cache limit reached; clear old files manually after pasting'})
                    key=secrets.token_hex(16);tmp=CACHE/(key+'.part');dest=CACHE/(key+'.jpg')
                    digest=hashlib.sha256();first=b'';tail=b''
                    try:
                        remaining=size
                        with tmp.open('xb') as f:
                            while remaining:
                                chunk=self.rfile.read(min(1024*1024,remaining))
                                if not chunk: raise ValueError('Incomplete upload')
                                if not first: first=chunk[:3]
                                tail=(tail+chunk)[-2:];digest.update(chunk);f.write(chunk);remaining-=len(chunk)
                        if first[:3]!=b'\xff\xd8\xff' or tail!=b'\xff\xd9': raise ValueError('JPEG signature invalid (not decoded by client)')
                        tmp.rename(dest)
                    finally:
                        if tmp.exists(): tmp.unlink()
                    FILES[key]=dest
                return self.respond(201,{'fileId':key,'name':dest.name,'bytes':size,'sha256':digest.hexdigest()})
            if size>4096: return self.respond(413,{'error':'Request too large'})
            body=json.loads(self.rfile.read(size))
            if self.path == '/api/clipboard/file':
                key=body.get('fileId');dest=FILES.get(key)
                if not dest or not dest.is_file(): return self.respond(404,{'error':'Unknown file ID'})
                with LOCK:
                    result=subprocess.run([str(HELPER),str(dest)],capture_output=True,text=True,timeout=15)
                if result.returncode: raise ValueError('Clipboard helper failed: '+result.stderr.strip())
                report=json.loads(result.stdout)
                if not report.get('verified'): raise ValueError('Clipboard helper did not verify file URL; restart client to rebuild helper')
                emit('clipboard', name=dest.name, bytes=dest.stat().st_size)
                return self.respond(200,report)
            if self.path == '/api/health':
                global LAST_ACTIVITY
                now=time.time()
                if now-LAST_ACTIVITY>5:
                    LAST_ACTIVITY=now;emit('connected', origin=self.headers.get('Origin'))
                return self.respond(200,{'version':2,'clipboard':'file-url','codec':False})
            return self.respond(404,{'error':'Not found'})
        except Exception as e: return self.respond(400,{'error':str(e)})

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--port',type=int,default=0);parser.add_argument('--no-open',action='store_true');parser.add_argument('--cache-dir',type=Path);parser.add_argument('--origin');parser.add_argument('--allow-file',action='store_true');parser.add_argument('--json-events',action='store_true');args=parser.parse_args()
    if args.origin: EXTRA_ORIGIN=url_origin(args.origin)
    ALLOW_FILE=args.allow_file
    if args.cache_dir: CACHE=args.cache_dir.resolve()
    CACHE.mkdir(parents=True,exist_ok=True,mode=0o700);os.chmod(CACHE,0o700)
    # Keep cache files across restarts; do not remove files still referenced by clipboard.
    os.umask(0o077)
    HELPER=Path(__file__).resolve().parent/'clipboard-helper'
    if not HELPER.is_file(): raise SystemExit('Compile helper first: ./start.command')
    server=ThreadingHTTPServer(('127.0.0.1',args.port),Handler)
    url=f'http://127.0.0.1:{server.server_port}/{RAIL}#client=http://127.0.0.1:{server.server_port}&token={TOKEN}'
    if args.json_events: emit('ready', endpoint=f'http://127.0.0.1:{server.server_port}', token=TOKEN)
    print(f'JPEG clipboard client listening on 127.0.0.1:{server.server_port}',flush=True)
    print('Use the browser window opened by this client. Ctrl+C stops the service. Cache retained for file pasting.',flush=True)
    if not args.no_open: webbrowser.open(url)
    try: server.serve_forever()
    except KeyboardInterrupt: pass
    finally: server.server_close()
