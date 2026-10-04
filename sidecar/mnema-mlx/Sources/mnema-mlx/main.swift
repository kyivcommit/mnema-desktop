import Foundation
import MnemaMLX

guard CommandLine.arguments.contains("--stub") else {
    FileHandle.standardError.write(Data("only --stub is supported\n".utf8))
    exit(2)
}
let env = ProcessInfo.processInfo.environment
let server = try Server(token: env["MNEMA_MLX_TOKEN"] ?? "", embedDelayMs: Int(env["MNEMA_STUB_EMBED_MS"] ?? "") ?? 0)
print("PORT \(try server.start())")
fflush(stdout)
_ = FileHandle.standardInput.readDataToEndOfFile()
