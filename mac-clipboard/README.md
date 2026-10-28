# macOS JPEG 文件剪贴板 · 5.6

配套网页 5.6.0；桥接协议 version=4（协议号不随产品版本递增）。

## 使用

打开构建后的 `JPEG Clipboard.app`，选择「内置压缩器」或「自定义压缩器 URL」，保存并重启，再打开压缩器。仅绑定 127.0.0.1:48731–48735，自动退避已占用端口。授权按协议、主机、端口匹配，不限制同源路径，无配对弹窗、令牌或 URL fragment。

内置网页移除第三方统计，记录仅保存在浏览器本地；外部网页由其自身隐私策略决定。压缩和 ZIP 打包由浏览器执行，客户端只验证上传、保存文件、写入 NSPasteboard 并回读校验。缓存位于 `~/Library/Caches/LocalJPEGClipboard`，上限 1GB；清理保留当前剪贴板引用文件，文件不是永久归档。

已移除 file:// 授权入口及服务端支持，Origin:null 一律拒绝，旧授权设置会清除。

关闭窗口仍在菜单栏运行；菜单栏退出停止服务。窗口按内容自适应高度，跟随系统深浅色。

## 构建与测试

```sh
bash mac-clipboard/build-app.command
cargo test --locked --manifest-path mac-rust-service/Cargo.toml
```

从 5.6 起 App 使用 Swift/AppKit 界面、Rust 原生后台与 Swift 文件剪贴板助手，运行时不再依赖 Python 或 Xcode Command Line Tools。网页资源直接编译进 Rust 后台。开发构建需要 Rust 与 Swift 编译器；分发 App 为 ad-hoc 签名，未做开发者公证。目前交付 Apple Silicon arm64 版本，Intel 需另行构建。旧 Python 服务与测试仅保留作参考，不打包进 App。

JPEG 只验证头尾标识及大小，不进行客户端解码。ZIP 验证中央目录、路径与声明体积后原样保存，不提取、不重打包，也不校验展开内容与 CRC；不替代杀毒软件。真实目标应用粘贴、跨机器兼容与长期稳定性仍需实机验收。

## 与 Windows 的保留差异

Windows 为 Rust + Win32 单 EXE，无需另装运行时，关闭窗口退出；Mac 为 Swift + Rust，支持菜单栏后台。Windows 串行 HTTP worker 对不完整上传缺少连接读超时，可能阻塞后续请求与重启；遇到此情况需关闭异常页面或重开客户端。两端 UI 使用各自原生控件，不追求像素级一致。
