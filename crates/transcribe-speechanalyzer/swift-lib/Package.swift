// swift-tools-version:5.9

import PackageDescription

let package = Package(
  name: "apple-speech-swift",
  platforms: [.macOS("14.2")],
  products: [
    .library(
      name: "apple-speech-swift",
      type: .static,
      targets: ["AppleSpeechBridge"])
  ],
  dependencies: [
    .package(
      url: "https://github.com/Brendonovich/swift-rs",
      revision: "01980f981bc642a6da382cc0788f18fdd4cde6df")
  ],
  targets: [
    .target(
      // Unique module name; see scripts/check-swift-modules.mjs.
      name: "AppleSpeechBridge",
      dependencies: [
        .product(name: "SwiftRs", package: "swift-rs")
      ],
      path: "src"
    )
  ]
)
