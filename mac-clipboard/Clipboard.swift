import AppKit
import Foundation
// Only file references: no image decoding or image representations.
let args = CommandLine.arguments
if args.count != 2 { fputs("Expected one file path\n", stderr); exit(2) }
let url = URL(fileURLWithPath: args[1]).standardizedFileURL
var isDirectory: ObjCBool = false
 guard FileManager.default.fileExists(atPath: url.path, isDirectory: &isDirectory), !isDirectory.boolValue else {
    fputs("File missing\n", stderr); exit(2)
}
let pb = NSPasteboard.general
pb.clearContents()
guard pb.writeObjects([url as NSURL]) else { fputs("Clipboard write failed\n", stderr); exit(1) }
let objects = pb.readObjects(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL] ?? []
guard objects.contains(where: { $0.standardizedFileURL == url }) else {
    fputs("Clipboard file URL read-back verification failed\n", stderr); exit(1)
}
let report: [String: Any] = ["written": true, "verified": true, "representation": "file-url", "name": url.lastPathComponent, "changeCount": pb.changeCount]
let data = try JSONSerialization.data(withJSONObject: report)
print(String(data: data, encoding: .utf8)!)
