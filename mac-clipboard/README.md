# macOS JPEG 文件剪贴板 · 5.4

配套网页 5.4.0；桥接协议 version=4（协议号不随产品版本递增）。

## 使用

打开构建后的 `JPEG Clipboard.app`，选择「内置压缩器」或「自定义压缩器 URL」，保存并重启，再打开压缩器。仅绑定 127.0.0.1:48731–48735，自动退避已占用端口。授权按协议、主机、端口匹配，不限制同源路径，无配对弹窗、令牌或 URL fragment。

内置网页移除第三方统计，记录仅保存在浏览器本地；外部网页由其自身隐私策略决定。压缩和 ZIP 打包由浏览器执行，客户端只验证上传、保存文件、写入 NSPasteboard 并回读校验。缓存位于 `~/Library/Caches/LocalJPEGClipboard`，上限 1GB；清理保留当前剪贴板引用文件，文件不是永久归档。

允许 file:// 为额外可选授权：所有不透明来源共享 Origin:null，无令牌隔离，不建议启用。「清除允许来源」同时撤销此项。

关闭窗口仍在菜单栏运行；菜单栏退出停止服务。窗口按内容自适应高度，跟随系统深浅色。

## 构建与测试

```sh
bash mac-clipboard/build-app.command
python3 -B mac-clipboard/test_server.py
python3 -B mac-clipboard/test_ports.py
```

App 依赖本机 `/usr/bin/python3`（通常来自 Xcode Command Line Tools），并非无运行时的独立二进制。构建脚本内嵌网页快照并执行 ad-hoc 签名，未做开发者公证。修改网页或服务后需重建 App。

JPEG 只验证头尾标识及大小，不进行客户端解码。ZIP 验证中央目录、路径与声明体积后原样保存，不提取、不重打包，也不校验展开内容与 CRC；不替代杀毒软件。真实目标应用粘贴、跨机器兼容与长期稳定性仍需实机验收。

## 与 Windows 的保留差异

Windows 为 Rust + Win32 单 EXE，无需另装运行时，关闭窗口退出；Mac 为 Swift + Python，支持菜单栏后台和额外 file:// 授权。Windows 串行 HTTP worker 对不完整上传缺少连接读超时，可能阻塞后续请求与重启；遇到此情况需关闭异常页面或重开客户端。两端 UI 使用各自原生控件，不追求像素级一致。
