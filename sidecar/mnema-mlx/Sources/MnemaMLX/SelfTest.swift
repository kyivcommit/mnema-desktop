import MLX

/// One matmul on the GPU, so the bundled metallib is actually loaded and a kernel runs.
/// A bundle whose sidecar is a placeholder, or whose metallib or Span runtime is missing, cannot print METAL OK.
public func metalSelfTest() -> Bool {
    let ones = MLXArray.ones([8, 8])
    let product = matmul(ones, ones, stream: .gpu)
    eval(product)
    return product[0, 0].item(Float.self) == 8
}
