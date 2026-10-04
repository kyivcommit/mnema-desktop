import Foundation
import MnemaMLX

// mnema-mlx --stub | --embed <bge-m3 dir> --chat <gemma dir>
let args = CommandLine.arguments
let env = ProcessInfo.processInfo.environment
let token = env["MNEMA_MLX_TOKEN"] ?? ""
guard !token.isEmpty else {
    FileHandle.standardError.write(Data("MNEMA_MLX_TOKEN is empty or not set; refusing to start\n".utf8))
    exit(2)
}
func arg(_ name: String) -> String? { args.firstIndex(of: name).flatMap { $0 + 1 < args.count ? args[$0 + 1] : nil } }

let server: Server
if args.contains("--stub") {
    server = try Server(token: token, embedDelayMs: Int(env["MNEMA_STUB_EMBED_MS"] ?? "") ?? 0)
} else if let embed = arg("--embed"), let chat = arg("--chat") {
    server = try Server(token: token, embedDir: URL(fileURLWithPath: embed), chatDir: URL(fileURLWithPath: chat))
} else {
    FileHandle.standardError.write(Data("usage: mnema-mlx --stub | --embed <dir> --chat <dir>\n".utf8))
    exit(2)
}
let port: UInt16
do { port = try server.start() } catch {
    FileHandle.standardError.write(Data("\(error)\n".utf8))
    exit(1)
}
print("PORT \(port)")
fflush(stdout)
_ = FileHandle.standardInput.readDataToEndOfFile()
