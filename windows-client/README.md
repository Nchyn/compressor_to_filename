# JPEG 剪贴板 Windows 原生预览版 5.4

Rust + Win32 原生窗口（`windows-sys`），不是 WebView。只接收网页已经生成的 JPEG / ZIP，将文件写入本机缓存，然后通过 `CF_HDROP` 文件剪贴板粘贴到资源管理器、聊天软件等支持文件粘贴的位置。没有图像编解码器、云端上传、令牌、配对或自动更新。

## 使用

1. 在 Windows 10 / 11 x64 上启动发布版 EXE。默认使用内置压缩器，页面资源编译进 EXE，浏览器通过本机 HTTP 打开；也可切换自定义 URL。
2. 若使用其他页面，输入完整 HTTPS URL，点击「保存并重启」。HTTP 只允许 `localhost`、`127.0.0.1`。**授权的是完整来源（协议、主机、端口），不是路径**；同来源下其他页面具有相同权限。
3. 点击「打开压缩器」，允许浏览器提示的本地网络访问。网页自动探测 `127.0.0.1:48731` 至 `48735`，无需参数或令牌。
4. 处理图片 / 文件夹后，在支持文件粘贴的 Windows 程序里 Ctrl+V。ZIP 保留网页上传的原始字节；不会解压、重打包或更改压缩格式。
5. 「打开缓存」查看文件。「清除缓存」须确认，服务停止后清理；当前 `CF_HDROP` 引用的文件会保留，剪贴板无法读取则拒绝清理。历史网页复制按钮在文件被清理后可能失效。

窗口带 URL 编辑框、保存重启 / 打开网页 / 启动 / 停止 / 打开缓存 / 清理按钮及可选择、Ctrl+A、Ctrl+C 的只读日志。关闭窗口即停止服务，不留托盘后台进程。停止 / 重启在后台等待正在处理的请求，窗口保持响应。缓存不会在退出时自动删除，避免剪贴板引用失效。服务重启后旧的 fileId 不再有效，但已有剪贴板文件仍可粘贴。

配置位于 `%LOCALAPPDATA%\JPEGClipboardWindows\compressor-url.txt`，缓存位于该目录下 `Cache`。无注册表配置。缓存文件属于当前用户的数据目录；同账户程序仍可访问，不是抵御本机恶意程序的隔离边界。

## 与现有网页桥接协议兼容

以 `mac-clipboard/server.py`、`ClientApp.swift`、`indexv2.html` 桥接实现为依据：

| 请求 | 行为 |
| --- | --- |
| `POST /api/health`，JSON `{}` | `service:"local-jpeg-clipboard", version:4, archives:true, clipboard:"file-url", codec:false` |
| `POST /api/files`，`image/jpeg` | `4 <= bytes < 20,000,000`，检查 `FF D8 FF` / `FF D9`，流式保存；返回 201 `fileId,name,bytes,sha256` |
| `POST /api/archives`，`application/zip` | `0 < bytes <= 250,000,000`；读取 ZIP 中央目录校验后原样保存；返回 201 `fileId,name,bytes,sha256` |
| `POST /api/clipboard/file`，JSON `{fileId}` | 仅允许本次服务注册的 ID；专用 STA 线程写入 `CF_HDROP` 后关闭、重新打开剪贴板回读；返回 `verified:true,written:true,format:"CF_HDROP",files:1` |
| `OPTIONS` | 来源与 Host 检查后 204；允许 `POST, OPTIONS`、`Authorization, Content-Type`，按浏览器请求回显 Private Network 授权 |

Host 必须精确为当前端口的 `127.0.0.1:端口`。Origin 必须精确匹配已保存 URL 的规范化来源，或该端口的 `http://127.0.0.1:端口`（与 Mac 桥一致）。不接受无 Origin、`null`、file://、任意 localhost Host 或通配符。CORS 按允许来源回显；响应禁止缓存。只监听 IPv4 回环，不对 LAN 提供服务。仅提供固定白名单静态资源，不暴露任意本机文件路径，不提供令牌或配对端点。内置页面禁用第三方统计和云端记录，记录仅保存在浏览器本机。

所有 POST 必须有 Content-Length，不接受 Transfer-Encoding；非上传 JSON 最大 4096 字节。上传串行化，持久缓存总量最大十进制 1GB。随机 128 位 ID；JPEG 名称不由请求决定；ZIP 推断唯一顶层目录名作为 ZIP 文件名，多根目录用「文件夹.zip」。Windows 非法字符、保留设备名、尾部空格和点会处理，ZIP 路径不会用于提取文件。

ZIP 校验限制中央目录最多 10,000 项、声明展开总量最多 250MB；拒绝加密、重复名、绝对路径、反斜杠、NUL、父级路径、盘符路径。只读取元数据，**不会检查解压后的 CRC / 内容，也不会提取文件**。这不是杀毒或解压沙箱。缓存目录及祖先、子项检测 Windows reparse point（符号链接 / junction 等），发现异常拒绝上传或跳过清理；仍假设本机同账户进程不在检查与文件操作之间恶意修改目录。

剪贴板专用线程初始化 COM STA，建立隐藏 Win32 owner 窗口并泵消息。OpenClipboard 失败重试最多约 1 秒。HGLOBAL 使用 `GMEM_MOVEABLE`，失败自行释放，SetClipboardData 成功后所有权交给系统，不再释放。API 返回成功前必须重新打开、读取并验证单个路径一致。

## 编译

安装当前 stable Rust。原生 Windows 推荐 MSVC 工具链（安装 Visual Studio Build Tools 的 Desktop development with C++）：

```powershell
rustup target add x86_64-pc-windows-msvc
$env:RUSTFLAGS='-C target-feature=+crt-static'
cargo build --locked --release --target x86_64-pc-windows-msvc
```

输出 `target/x86_64-pc-windows-msvc/release/jpeg-clipboard-windows.exe`。

交叉编译 GNU 目标可选 MinGW-w64：

```sh
rustup target add x86_64-pc-windows-gnu
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc
RUSTFLAGS='-C target-feature=+crt-static' cargo build --locked --release --target x86_64-pc-windows-gnu
```

或安装 Zig + cargo-zigbuild 后：

```sh
cargo zigbuild --locked --release --target x86_64-pc-windows-gnu
```

部分 Zig / Rust 组合需处理 MinGW import libraries 和 linker 参数；仓库另提供本项目构建脚本作为已准备工具链的入口。依赖仅 Win32 绑定、tiny_http、URL 解析、JSON、SHA-256、OS 随机数、ZIP 元数据；未启用 ZIP 压缩算法依赖。Cargo.lock 固定依赖版本。

Windows 上可执行单元测试：

```powershell
cargo test --locked --target x86_64-pc-windows-msvc
```

## 原型限制与验收

- **tiny_http 0.12 没有公开每连接读超时配置。** 对本机已获准来源或其他本机程序发送的慢速 / 不完整请求，串行 HTTP worker 可能等待读取完成，停止 / 重启也可能等待该连接。UI 不阻塞，关闭窗口仍可退出；必要时关闭发送页面或重开客户端。这是明确的本地拒绝服务限制，不宣称已解决 Slowloris。请求大小和缓存配额仍被限制。
- GUI 是固定尺寸原生控件，未做完整高 DPI、多显示器、无障碍、托盘、单实例、国际化或签名安装器。状态 / 错误日志部分使用英文。Windows SmartScreen 可能提示未签名应用。
- HTTP / 剪贴板代码需要真实 Windows 10/11 验收，交叉编译成功不等于运行验证：测试 JPEG 文件粘贴、包含中文文件名的 ZIP、空文件夹、重复客户端端口回退、来源拒绝、Host 拒绝、剪贴板被其他进程占用、清缓存保留当前文件、重启后既有剪贴板仍可粘贴。
- 文件保留由显式清缓存控制。若 1GB 配额不足，先完成粘贴再清理，不自动驱逐仍可能需要的文件。
