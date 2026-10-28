# compressor_to_filename

`indexv2.html` 为右栏设置版压缩器，保留文件夹/ZIP批量压缩，并支持网页JPEG编码及macOS文件剪贴板。

## 网页
通过静态服务器或既有网站访问 `indexv2.html`。所有图片编解码在浏览器中完成。

## 原生客户端 5.6

Mac：Swift/AppKit 界面 + Rust 后台，无 Python 运行时依赖；Windows：Rust/Win32 单 EXE。两端最近复制记录统一使用十进制 KB（1KB=1000字节）。内置网页 5.6.0 的图片处理在浏览器本机完成。

## macOS客户端
源码、构建和授权说明见 [mac-clipboard/README.md](mac-clipboard/README.md)。

```bash
bash mac-clipboard/build-app.command
open 'mac-clipboard/JPEG Clipboard.app'
```

应用只接收已编码的JPEG、保存到本机缓存并写入文件URL剪贴板，不编解码、不将剪贴板内容同步D1。默认保持图片尺寸，目标JPEG严格小于20,000,000字节；40MB输入减半为可选项。模式旁绿点表示客户端连接成功；粘贴结果说明列提供手动复制文件按钮。

自定义线上压缩器需在客户端UI授权其Origin后通过“打开压缩器”进入；浏览器可能要求本地网络访问权限。只授权自己信任的站点。不要将临时令牌、Cloudflare密钥或用户图片提交到仓库。

## 验证
```bash
cargo test --locked --manifest-path mac-rust-service/Cargo.toml
```

原生App、缓存和编译产物不提交；发布安装包应通过GitHub Releases。当前未配置新增部署流程，不改动现有站点或Cloudflare后端部署。
