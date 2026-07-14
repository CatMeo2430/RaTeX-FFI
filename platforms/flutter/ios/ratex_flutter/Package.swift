// swift-tools-version: 5.9

import PackageDescription

let package = Package(
    name: "ratex_flutter",
    platforms: [
        .iOS(.v13),
    ],
    products: [
        .library(name: "ratex-flutter", targets: ["ratex_flutter"]),
    ],
    dependencies: [
        .package(name: "FlutterFramework", path: "../FlutterFramework"),
    ],
    targets: [
        .target(
            name: "ratex_flutter",
            dependencies: [
                .product(name: "FlutterFramework", package: "FlutterFramework"),
                "ratex_flutter_linker",
            ],
            path: "Sources/ratex_flutter"
        ),
        .target(
            name: "ratex_flutter_linker",
            dependencies: [
                "ratex_flutter_ffi",
            ],
            path: "Sources/ratex_flutter_linker"
        ),
        .binaryTarget(
            name: "ratex_flutter_ffi",
            path: "RaTeX.xcframework"
        ),
    ]
)
