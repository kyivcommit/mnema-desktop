// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "mnema-mlx",
    platforms: [.macOS(.v14)],
    targets: [
        .target(name: "MnemaMLX"),
        .executableTarget(name: "mnema-mlx", dependencies: ["MnemaMLX"]),
        .testTarget(name: "MnemaMLXTests"),
    ]
)
