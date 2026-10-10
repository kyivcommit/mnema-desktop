// swift-tools-version:5.9
import PackageDescription

// Exact pins: mlx-swift is the revision the Metal 3.1 patch (patches/mlx-metal31.patch) was checked on;
// Package.resolved is versioned. Build and test only with xcodebuild, never `swift build`: SwiftPM from
// the command line does not compile MLX's Metal shaders.
let package = Package(
    name: "mnema-mlx",
    platforms: [.macOS(.v14)],
    dependencies: [
        .package(url: "https://github.com/ml-explore/mlx-swift", revision: "19601207e9a0de51e03ee6ec0c3c5f3784275075"),
        .package(url: "https://github.com/ml-explore/mlx-swift-lm", revision: "5e46681b2adcef2db158e7b949aeae3896778e23"),
        .package(url: "https://github.com/huggingface/swift-huggingface", exact: "0.12.0"),
        .package(url: "https://github.com/huggingface/swift-transformers", exact: "1.3.4"),
    ],
    targets: [
        .target(name: "MnemaMLX", dependencies: [
            .product(name: "MLX", package: "mlx-swift"),
            .product(name: "MLXEmbedders", package: "mlx-swift-lm"),
            .product(name: "MLXLLM", package: "mlx-swift-lm"),
            .product(name: "MLXLMCommon", package: "mlx-swift-lm"),
            .product(name: "MLXHuggingFace", package: "mlx-swift-lm"),
            .product(name: "Tokenizers", package: "swift-transformers"),
        ]),
        .executableTarget(name: "mnema-mlx", dependencies: ["MnemaMLX"]),
        .testTarget(name: "MnemaMLXTests", dependencies: [
            "MnemaMLX",
            .product(name: "MLX", package: "mlx-swift"),
        ], resources: [.copy("Fixtures")]),
    ]
)
