import Cocoa

final class ClientApp: NSObject, NSApplicationDelegate, NSWindowDelegate {
    var window:NSWindow!, statusItem:NSStatusItem!, process:Process?, pipe:Pipe?, input:Pipe?
    var endpoint="", token="", buffer=Data(), lastSeen:Date?
    let state=NSTextField(labelWithString:"服务未启动")
    let connection=NSTextField(labelWithString:"网页未连接")
    let activity=NSTextField(wrappingLabelWithString:"尚无文件复制")
    let url=NSTextField(string:UserDefaults.standard.string(forKey:"compressorURL") ?? "")
    let external=NSButton(radioButtonWithTitle:"自定义压缩器URL",target:nil,action:nil)
    let local=NSButton(radioButtonWithTitle:"内置压缩器",target:nil,action:nil)
    let fileAccess=NSButton(checkboxWithTitle:"允许本地file://页面（所有不透明来源仍需令牌）",target:nil,action:nil)
    var root:URL { Bundle.main.resourceURL! }
    func applicationDidFinishLaunching(_ notification:Notification) {
        NSApp.setActivationPolicy(.regular)
        statusItem=NSStatusBar.system.statusItem(withLength:NSStatusItem.variableLength)
        statusItem.button?.title="◉ JPEG"
        let menu=NSMenu();menu.addItem(withTitle:"打开控制台",action:#selector(show),keyEquivalent:"").target=self
        menu.addItem(withTitle:"打开压缩器",action:#selector(openPage),keyEquivalent:"").target=self
        menu.addItem(.separator());menu.addItem(withTitle:"退出",action:#selector(quit),keyEquivalent:"q").target=self;statusItem.menu=menu
        let appMenu=NSMenu(), appItem=NSMenuItem();appMenu.addItem(appItem);let submenu=NSMenu();submenu.addItem(withTitle:"退出JPEG客户端",action:#selector(quit),keyEquivalent:"q").target=self;appItem.submenu=submenu;NSApp.mainMenu=appMenu
        window=NSWindow(contentRect:NSRect(x:0,y:0,width:650,height:520),styleMask:[.titled,.closable,.miniaturizable],backing:.buffered,defer:false)
        window.title="JPEG 剪贴板 · v0.4.0 · 直连";window.center();window.delegate=self
        let stack=NSStackView();stack.orientation = .vertical;stack.alignment = .leading;stack.spacing=14;stack.translatesAutoresizingMaskIntoConstraints=false
        window.contentView!.addSubview(stack)
        NSLayoutConstraint.activate([stack.leadingAnchor.constraint(equalTo:window.contentView!.leadingAnchor,constant:28),stack.trailingAnchor.constraint(equalTo:window.contentView!.trailingAnchor,constant:-28),stack.topAnchor.constraint(equalTo:window.contentView!.topAnchor,constant:24)])
        let heading=NSTextField(labelWithString:"本机文件剪贴板");heading.font = .boldSystemFont(ofSize:24);stack.addArrangedSubview(heading)
        stack.addArrangedSubview(state);stack.addArrangedSubview(connection)
        let sep=NSBox();sep.boxType = .separator;stack.addArrangedSubview(sep);sep.widthAnchor.constraint(equalTo:stack.widthAnchor).isActive=true
        local.target=self;local.action=#selector(selectLocal);external.target=self;external.action=#selector(selectExternal)
        let custom=UserDefaults.standard.bool(forKey:"external");local.state=custom ? .off:.on;external.state=custom ? .on:.off
        stack.addArrangedSubview(local);stack.addArrangedSubview(external);url.placeholderString="https://example.com/compressor.html";stack.addArrangedSubview(url);url.widthAnchor.constraint(equalTo:stack.widthAnchor).isActive=true
        fileAccess.state=UserDefaults.standard.bool(forKey:"allowFile") ? .on:.off;stack.addArrangedSubview(fileAccess)
        let help=NSTextField(wrappingLabelWithString:"保存压缩器URL后直接连接，无令牌或配对弹窗。仅允许该URL的来源（协议＋主机＋端口），不限制路径。请只填写可信网站。")
        help.textColor = .secondaryLabelColor;help.font = .systemFont(ofSize:12);stack.addArrangedSubview(help);help.widthAnchor.constraint(equalTo:stack.widthAnchor).isActive=true
        let buttons=NSStackView();buttons.spacing=10
        for (title,action) in [("保存并重启",#selector(save)),("打开压缩器",#selector(openPage)),("停止服务",#selector(stop)),("打开缓存",#selector(openCache)),("清除允许来源",#selector(revoke))] {let b=NSButton(title:title,target:self,action:action);buttons.addArrangedSubview(b)}
        stack.addArrangedSubview(buttons);stack.addArrangedSubview(activity);activity.widthAnchor.constraint(equalTo:stack.widthAnchor).isActive=true
        let note=NSTextField(wrappingLabelWithString:"客户端不编解码、不上传云端。只保存网页生成的JPEG并写文件剪贴板。关闭窗口仍在菜单栏运行。")
        note.textColor = .secondaryLabelColor;stack.addArrangedSubview(note);note.widthAnchor.constraint(equalTo:stack.widthAnchor).isActive=true
        Timer.scheduledTimer(withTimeInterval:3,repeats:true){[weak self] _ in guard let self=self else{return};if let seen=self.lastSeen,Date().timeIntervalSince(seen)>35 {self.connection.stringValue="网页未连接（等待重新打开）"}}
        show();start()
    }
    @objc func selectLocal(){local.state = .on;external.state = .off}
    @objc func selectExternal(){local.state = .off;external.state = .on}
    func alert(_ message:String){let a=NSAlert();a.messageText=message;a.runModal()}
    func validatedURL()->URL? {
        if external.state != .on{return nil}
        guard let u=URL(string:url.stringValue.trimmingCharacters(in:.whitespacesAndNewlines)) else{return nil}
        if u.scheme=="file" {return fileAccess.state == .on ? u:nil}
        guard u.user==nil,u.password==nil,u.host != nil else{return nil}
        if u.scheme=="https" || (u.scheme=="http" && ["127.0.0.1","localhost"].contains(u.host!)){return u};return nil
    }
    @objc func save(){
        if external.state == .on && validatedURL()==nil {alert("请输入HTTPS地址、本机HTTP地址，或勾选允许file://后选择本地地址");return}
        UserDefaults.standard.set(url.stringValue,forKey:"compressorURL");UserDefaults.standard.set(external.state == .on,forKey:"external");UserDefaults.standard.set(fileAccess.state == .on,forKey:"allowFile")
        stop();start()
    }
    func start(){
        guard process==nil else{return}
        let p=Process(),output=Pipe(),commands=Pipe();p.executableURL=URL(fileURLWithPath:"/usr/bin/python3")
        var args=["-u","-B",root.appendingPathComponent("mac-clipboard/server.py").path,"--no-open","--json-events"]
        if fileAccess.state == .on{args.append("--allow-file")}
        if let u=validatedURL(),u.scheme != "file" {args += ["--origin",u.absoluteString]}
        p.arguments=args;p.standardOutput=output;p.standardError=output;p.standardInput=commands;input=commands
        buffer=Data();process=p;pipe=output;state.stringValue="启动中…"
        output.fileHandleForReading.readabilityHandler={[weak self,weak p] handle in let data=handle.availableData;if data.isEmpty{return};DispatchQueue.main.async{guard let self=self,self.process===p else{return};self.receive(data)}}
        p.terminationHandler={[weak self,weak p] _ in DispatchQueue.main.async{guard let self=self,self.process===p else{return};self.process=nil;self.endpoint="";self.token="";self.state.stringValue="服务已退出，请保存并重启";self.connection.stringValue="网页未连接"}}
        do{try p.run()}catch{process=nil;state.stringValue="启动失败：\(error.localizedDescription)"}
    }
    func receive(_ data:Data){
        buffer.append(data)
        while let range=buffer.range(of:Data([10])) {let line=buffer.subdata(in:0..<range.lowerBound);buffer.removeSubrange(0..<range.upperBound)
            guard let object=(try? JSONSerialization.jsonObject(with:line)) as? [String:Any],let event=object["event"] as? String else {continue}
            if event=="ready" {endpoint=object["endpoint"] as? String ?? "";token=object["token"] as? String ?? "";state.stringValue="● 服务运行中 · \(endpoint)";state.textColor = .systemGreen;connection.stringValue="等待网页连接"}
            else if event=="connected" {lastSeen=Date();connection.stringValue="● 网页已连接 · \(object["origin"] as? String ?? "")";connection.textColor = .systemGreen}
            else if event=="clipboard" {activity.stringValue="已复制：\(object["name"] as? String ?? "")\n\(object["bytes"] ?? 0) 字节 · \(DateFormatter.localizedString(from:Date(),dateStyle:.none,timeStyle:.medium))"}
        }
    }
    @objc func stop(){let old=process;process=nil;old?.terminate();pipe?.fileHandleForReading.readabilityHandler=nil;pipe=nil;endpoint="";token="";lastSeen=nil;state.stringValue="服务已停止";state.textColor = .secondaryLabelColor;connection.stringValue="网页未连接"}
    @objc func openPage(){
        if (external.state == .on) != UserDefaults.standard.bool(forKey:"external") || (external.state == .on && url.stringValue != (UserDefaults.standard.string(forKey:"compressorURL") ?? "")) || (fileAccess.state == .on) != UserDefaults.standard.bool(forKey:"allowFile") {alert("授权设置已更改，请先点击保存并重启");return}
        guard !endpoint.isEmpty else {alert("服务尚未就绪，请先保存授权并启动");return}
        var target:URL
        if external.state == .on {guard let u=validatedURL() else{alert("授权URL无效");return};target=u}else{target=URL(string:endpoint+"/indexv2.html")!}
        NSWorkspace.shared.open(target)
    }
    func send(_ value:[String:Any]){if let data=try? JSONSerialization.data(withJSONObject:value){try? input?.fileHandleForWriting.write(contentsOf:data+Data([10]))}}
    @objc func revoke(){stop();url.stringValue="";selectLocal();UserDefaults.standard.set("",forKey:"compressorURL");UserDefaults.standard.set(false,forKey:"external");start()}
    @objc func openCache(){let folder=FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Caches/LocalJPEGClipboard");try? FileManager.default.createDirectory(at:folder,withIntermediateDirectories:true);NSWorkspace.shared.open(folder)}
    @objc func show(){window.makeKeyAndOrderFront(nil);NSApp.activate(ignoringOtherApps:true)}
    @objc func quit(){stop();NSApp.terminate(nil)}
    func applicationWillTerminate(_ notification:Notification){stop()}
    func applicationShouldHandleReopen(_ sender:NSApplication,hasVisibleWindows flag:Bool)->Bool{show();return true}
    func applicationShouldTerminateAfterLastWindowClosed(_ sender:NSApplication)->Bool{false}
}
let app=NSApplication.shared
let delegate=ClientApp();app.delegate=delegate;app.run()
