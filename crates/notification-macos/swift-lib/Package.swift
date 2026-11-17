// swift-tools-version:5.9

import PackageDescription

let package = Package(
  name: "notification-swift",
  platforms: [.macOS("14.2")],
  products: [
    .library(
      name: "notification-swift",
      type: .static,
      targets: ["NotificationBridge"])
  ],
  dependencies: [
    .package(
      url: "https://github.com/Brendonovich/swift-rs",
      revision: "01980f981bc642a6da382cc0788f18fdd4cde6df")
  ],
  targets: [
    .target(
      // Unique module name; see scripts/check-swift-modules.mjs.
      name: "NotificationBridge",
      dependencies: [
        .product(name: "SwiftRs", package: "swift-rs")
      ],
      path: "src"
    ),
    .testTarget(
      name: "NotificationBridgeTests",
      dependencies: ["NotificationBridge"],
      path: "tests"
    ),
  ]
)
