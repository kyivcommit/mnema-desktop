import Foundation
import MnemaMLX

// mnema-mlx --stub | --embed <bge-m3 dir> --chat <gemma dir>
let args = CommandLine.arguments
let env = ProcessInfo.processInfo.environment
let token = env["MNEMA_MLX_TOKEN"] ?? ""
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
print("PORT \(try server.start())")
fflush(stdout)
_ = FileHandle.standardInput.readDataToEndOfFile()
